//! Federated recall's pure rules (ADR-0072 phase 1): which workareas are
//! trusted sources, the size-aware estimate that orders candidates above the
//! cap, the tail's lower bound, and the widen hint's rate test. Every
//! function is a function of exact counts and names alone: no clock, no
//! randomness and no model (d9), so the same counts give the same order.

use crate::ladder::WorkareaStatus;

/// The tenant's `max_sources` when no admin set one (d5; unmeasured, #205).
pub const MAX_SOURCES_DEFAULT: u32 = 8;

/// The largest `max_sources` an admin may set: the most names a `sources`
/// list may carry, and the most sources a `wide` recall searches beside its
/// scope.
pub const MAX_SOURCES_CEILING: u32 = 64;

/// Above this many candidate sources a key is named by a doctor line to a
/// tenant admin; a recall is never refused for its count (d12).
pub const CANDIDATES_DOCTOR: usize = 1_000;

/// The widen hint's rate factor: a candidate at least this many times richer
/// per readable row than the scope fires it (d8). Fixed, not tuned.
pub const WIDEN_FACTOR: f64 = 2.0;

/// The widen hint's floor: a candidate fires it only with a kept word held
/// by at least this many of its rows, so one stray row never does (d8).
pub const WIDEN_FLOOR: u64 = 2;

/// Whether a node is a trusted source (d1): `committed` or `validated`, and
/// outside every in-progress subtree. A `development` node never is, and
/// every node of an in-progress subtree is `development` by construction
/// (ADR-0030 d1), so it is excluded twice. Judged per node, never per
/// subtree.
#[must_use]
pub fn is_trusted_source(status: WorkareaStatus, inside_in_progress: bool) -> bool {
    matches!(
        status,
        WorkareaStatus::Committed | WorkareaStatus::Validated
    ) && !inside_in_progress
}

/// The non-negative BM25 inverse document frequency over a reference set of
/// `n` rows of which `df` hold the word: `ln(1 + (n − df + 0.5) / (df + 0.5))`
/// (d4.4), the form every BM25 statement scores with.
#[must_use]
pub fn idf(n: u64, df: u64) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let (n, df) = (n as f64, df as f64);
    (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
}

/// One source's exact counts over the query's kept words: its served rows
/// and, per kept word in the query's order, how many of them hold it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceCounts {
    pub rows: u64,
    pub df: Vec<u64>,
}

impl SourceCounts {
    /// The sum of several sources' counts, word by word.
    #[must_use]
    pub fn sum<'a>(words: usize, of: impl IntoIterator<Item = &'a SourceCounts>) -> Self {
        let mut total = SourceCounts {
            rows: 0,
            df: vec![0; words],
        };
        for s in of {
            total.rows += s.rows;
            for (t, d) in total.df.iter_mut().zip(&s.df) {
                *t += d;
            }
        }
        total
    }
}

/// E(s) (d4.4): the share of the reference set's matching rows the source
/// holds, weighted by each kept word's rarity in the reference set,
/// `Σ_w IDF_R(w) · df_s(w) / df_R(w)`; a word no reference row holds adds
/// nothing. No tuned constant; summed in the words' order, so the same
/// counts give the same float.
#[must_use]
pub fn estimate(source: &SourceCounts, reference: &SourceCounts) -> f64 {
    let mut e = 0.0;
    for (i, &df_r) in reference.df.iter().enumerate() {
        let df_s = source.df.get(i).copied().unwrap_or(0);
        if df_r == 0 || df_s == 0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
        let share = df_s as f64 / df_r as f64;
        e += idf(reference.rows, df_r) * share;
    }
    e
}

/// ρ(s) (d8): the source's matches per readable row weighted by rarity,
/// `Σ_w IDF_R(w) · df_s(w) / n_s`, the same weights as [`estimate`] over the
/// source's own size; 0 for a source with no rows.
#[must_use]
pub fn rate(source: &SourceCounts, reference: &SourceCounts) -> f64 {
    if source.rows == 0 {
        return 0.0;
    }
    let mut weighted = 0.0;
    for (i, &df_r) in reference.df.iter().enumerate() {
        let df_s = source.df.get(i).copied().unwrap_or(0);
        if df_r == 0 || df_s == 0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
        let held = df_s as f64;
        weighted += idf(reference.rows, df_r) * held;
    }
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let rows = source.rows as f64;
    weighted / rows
}

/// Whether a candidate is a richer source than the scope (d8.1): its rate is
/// at least [`WIDEN_FACTOR`] times the scope's, and some kept word is held
/// by at least [`WIDEN_FLOOR`] of its rows. With a scope rate of 0 any
/// candidate meeting the floor is richer.
#[must_use]
pub fn richer(candidate: &SourceCounts, scope_rate: f64, reference: &SourceCounts) -> bool {
    candidate.df.iter().any(|&d| d >= WIDEN_FLOOR)
        && rate(candidate, reference) >= WIDEN_FACTOR * scope_rate
}

/// The lower bound on the rows of `sources` that hold a kept word (d4.6):
/// the maximum over words of the sum of their counts. Exact counts never
/// overlap across sources, so the bound holds; it may undercount rows that
/// hold several words.
#[must_use]
pub fn matches_at_least<'a>(
    words: usize,
    sources: impl IntoIterator<Item = &'a SourceCounts>,
) -> u64 {
    SourceCounts::sum(words, sources)
        .df
        .into_iter()
        .max()
        .unwrap_or(0)
}

/// The candidates in selection order (d4.4): by estimate descending, then by
/// name ascending in byte order, names being tenant-unique and immutable
/// (ADR-0026). A candidate with an estimate of 0 holds no kept word and
/// sorts last, by name.
pub fn order_by_estimate<T>(candidates: &mut [(T, String, f64)]) {
    candidates.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| a.1.as_bytes().cmp(b.1.as_bytes()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(rows: u64, df: &[u64]) -> SourceCounts {
        SourceCounts {
            rows,
            df: df.to_vec(),
        }
    }

    #[test]
    fn only_committed_and_validated_nodes_outside_an_in_progress_subtree_are_trusted() {
        assert!(is_trusted_source(WorkareaStatus::Committed, false));
        assert!(is_trusted_source(WorkareaStatus::Validated, false));
        assert!(!is_trusted_source(WorkareaStatus::Development, false));
        assert!(!is_trusted_source(WorkareaStatus::Committed, true));
    }

    #[test]
    fn a_small_source_holding_most_of_a_rare_words_rows_outranks_a_large_one_holding_one() {
        // d4.4's example: 300 rows holding five of the six rows with a rare
        // word outrank `universe` holding the sixth.
        let crew = counts(300, &[5]);
        let universe = counts(100_000, &[1]);
        let reference = SourceCounts::sum(1, [&crew, &universe]);
        assert!(estimate(&crew, &reference) > estimate(&universe, &reference));
    }

    #[test]
    fn a_word_no_reference_row_holds_adds_nothing_to_the_estimate() {
        let reference = counts(10, &[0, 2]);
        assert!(estimate(&counts(5, &[0, 0]), &reference).abs() < f64::EPSILON);
        assert!(estimate(&counts(5, &[0, 1]), &reference) > 0.0);
    }

    #[test]
    fn ties_on_the_estimate_break_by_name_and_an_estimate_of_zero_sorts_last() {
        let mut c = vec![
            ((), "zeta".to_owned(), 0.0),
            ((), "beta".to_owned(), 1.0),
            ((), "alpha".to_owned(), 1.0),
            ((), "gamma".to_owned(), 2.0),
        ];
        order_by_estimate(&mut c);
        let names: Vec<&str> = c.iter().map(|x| x.1.as_str()).collect();
        assert_eq!(names, ["gamma", "alpha", "beta", "zeta"]);
    }

    #[test]
    fn the_hint_fires_on_a_source_twice_as_rich_and_never_on_one_stray_row() {
        let reference = counts(1_000, &[20]);
        let scope = counts(100, &[2]);
        let equal = counts(100, &[2]);
        let twice = counts(100, &[4]);
        let stray = counts(1, &[1]);
        let scope_rate = rate(&scope, &reference);
        assert!(!richer(&equal, scope_rate, &reference));
        assert!(richer(&twice, scope_rate, &reference));
        assert!(!richer(&stray, scope_rate, &reference));
        assert!(richer(&twice, 0.0, &reference));
    }

    #[test]
    fn the_tails_lower_bound_is_the_largest_summed_word_count() {
        let a = counts(10, &[3, 1]);
        let b = counts(10, &[2, 4]);
        assert_eq!(matches_at_least(2, [&a, &b]), 5);
        assert_eq!(matches_at_least(2, []), 0);
    }
}
