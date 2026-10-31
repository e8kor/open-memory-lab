//! Recall budgets and the recall order — pure functions over counts and
//! ranks that the app has already fetched (ADR-0025 d6, d7, d8;
//! ADR-0030 d8, d9, proposed; ADR-0019 d1).
//!
//! The rules are declared on `docs/architecture/retrieval.md`: "Token
//! budgets" (the budget token and the cut), "The recall-ordering contract"
//! (segment 1's structural order and the hits' total order) and "Held claims
//! inside an in-progress workarea" (held hits merged under that order). This
//! module mirrors them and adds nothing.

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::envelope::EventTime;

/// The counter id every budgeted response carries. Changing
/// [`budget_tokens`] mints a new id; `oml-tc/1` never changes meaning
/// (ADR-0025 d6).
pub const COUNTER_ID: &str = "oml-tc/1";

/// A row's count in budget tokens under counter `oml-tc/1`:
/// ⌈`body_bytes` ÷ 4⌉, where `body_bytes` is the UTF-8 octet length of the
/// row's NFC-normalized body (retrieval.md, "Token budgets"; ADR-0025 d6).
/// Only the body is counted, never ids, paths or metadata; the divisor is a
/// convention, not any model's tokenizer ratio.
#[must_use]
pub fn budget_tokens(body_bytes: u64) -> u64 {
    body_bytes.div_ceil(4)
}

/// How one `max_tokens` cut was applied: the budget object of
/// retrieval.md, "Token budgets", whose `counter` member is
/// [`COUNTER_ID`], plus `hits_served`, the served prefix length the app
/// hydrates, which is not a wire member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub max_tokens: u64,
    /// Σ of the steering counts; steering is never cut.
    pub steering_tokens: u64,
    /// `steering_tokens` plus the served hits' counts. Exceeds `max_tokens`
    /// only when steering alone does.
    pub used_tokens: u64,
    /// Whether any hit was cut.
    pub truncated: bool,
    /// j: the hits served are `hits[..j]`.
    pub hits_served: usize,
    pub hits_omitted: usize,
    /// The count of the first hit not served; `None` when nothing was cut.
    pub next_hit_tokens: Option<u64>,
}

/// The budget cut over the steering counts, the `k`-capped ranked hit
/// counts (own held rows included when asked) and the budget
/// (retrieval.md, "Token budgets"; ADR-0025 d7):
///
/// 1. s = Σ steering; steering is never cut;
/// 2. remaining = max(M − s, 0);
/// 3. the hits served are the longest prefix whose sum is at most
///    remaining — the first hit that does not fit ends the list, with no
///    skip-ahead and no partial body;
/// 4. truncated = (j < |hits|).
///
/// With no `max_tokens` there is no budget: the answer is `None`, every hit
/// is served, and the response carries no budget object.
#[must_use]
pub fn cut(steering: &[u64], hits: &[u64], max_tokens: Option<u64>) -> Option<Budget> {
    let max_tokens = max_tokens?;
    let steering_tokens = steering
        .iter()
        .fold(0, |sum: u64, &t| sum.saturating_add(t));
    let remaining = max_tokens.saturating_sub(steering_tokens);
    let mut served = 0_u64;
    let mut hits_served = 0;
    for &tokens in hits {
        match served.checked_add(tokens) {
            Some(sum) if sum <= remaining => {
                served = sum;
                hits_served += 1;
            }
            _ => break,
        }
    }
    Some(Budget {
        max_tokens,
        steering_tokens,
        used_tokens: steering_tokens.saturating_add(served),
        truncated: hits_served < hits.len(),
        hits_served,
        hits_omitted: hits.len() - hits_served,
        next_hit_tokens: hits.get(hits_served).copied(),
    })
}

/// Orders applicable steering structurally, never by the query
/// (retrieval.md, the recall-ordering contract; ADR-0025 d8): depth from
/// the `universe` root ascending, then `promoted_at` ascending, then
/// `artifact_id` ascending in byte order. `position` names a row's
/// (depth, `promoted_at`, `artifact_id`). `promoted_at` compares as its
/// signed string, which is fixed-width UTC (chain-format.md), so string
/// order is time order.
pub fn steering_order<T>(rows: &mut [T], position: impl Fn(&T) -> (u32, &EventTime, Uuid)) {
    rows.sort_by(|a, b| {
        let (a_depth, a_at, a_id) = position(a);
        let (b_depth, b_at, b_id) = position(b);
        a_depth
            .cmp(&b_depth)
            .then_with(|| a_at.as_str().cmp(b_at.as_str()))
            .then_with(|| a_id.cmp(&b_id))
    });
}

/// Orders hits by the total order of the recall-ordering contract
/// (retrieval.md; ADR-0025 d8): score descending, then `artifact_id`
/// ascending in byte order. `rank` names a row's (score, `artifact_id`); the
/// score is the store's `float4` rank, compared under IEEE 754 total order.
pub fn hit_order<T>(rows: &mut [T], rank: impl Fn(&T) -> (f32, Uuid)) {
    rows.sort_by(|a, b| {
        let (a_score, a_id) = rank(a);
        let (b_score, b_id) = rank(b);
        b_score.total_cmp(&a_score).then_with(|| a_id.cmp(&b_id))
    });
}

/// The hits a recall serves, from its promoted hit candidates and its held
/// ones — the key's own held rows under `include_own_held` (ADR-0025 d4)
/// and, at a scope inside an in-progress subtree, the held claims of other
/// keys under `include_unreviewed` (ADR-0030 d8, proposed): one list under
/// the total order of [`hit_order`], each version once, cut to `k`, then
/// cut by [`cut`] against `max_tokens` with steering counted first and never
/// cut (retrieval.md, the recall-ordering contract, "Held claims inside an
/// in-progress workarea" and "Token budgets"; ADR-0025 d7, d8). The order
/// reads a hit's score and `artifact_id`, never its state; inside an
/// in-progress subtree a held version scores as it will once promoted
/// (ADR-0030 d9), so there ratification changes neither the order nor the
/// cut.
///
/// `rank` names a candidate's (score, `artifact_id`). A version that arrives
/// in both held lists, a key's own held row inside, is served once, at its
/// first place in the order. `tokens` names a candidate's [`budget_tokens`]
/// count. The answer is the served hits in order and the budget object of
/// [`cut`], `None` without `max_tokens`.
pub fn merge_hits<T>(
    promoted: Vec<T>,
    held: Vec<T>,
    k: usize,
    steering: &[u64],
    max_tokens: Option<u64>,
    rank: impl Fn(&T) -> (f32, Uuid),
    tokens: impl Fn(&T) -> u64,
) -> (Vec<T>, Option<Budget>) {
    let mut hits = promoted;
    hits.extend(held);
    hit_order(&mut hits, &rank);
    let mut seen = BTreeSet::new();
    hits.retain(|hit| seen.insert(rank(hit).1));
    hits.truncate(k);
    let counts: Vec<u64> = hits.iter().map(tokens).collect();
    let budget = cut(steering, &counts, max_tokens);
    if let Some(budget) = &budget {
        hits.truncate(budget.hits_served);
    }
    (hits, budget)
}
