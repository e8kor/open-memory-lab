//! A workarea's brief as pure functions (ADR-0046 d7, d8): the fixed label
//! of a brief edge, the order its ratified members are served in, and the
//! token budget that cuts them under recall's prefix rule
//! (retrieval.md, "Token budgets").

use uuid::Uuid;

use crate::budget::{Budget, cut};

/// The one label a brief edge carries; it names the edge kind, and a claim
/// edge never carries it (ADR-0046 d6, d7).
pub const BRIEF_LABEL: &str = "brief";

/// `brief.max_tokens`'s default, in budget tokens of counter `oml-tc/1`.
/// Unmeasured: no session has been timed against it (#145).
pub const BRIEF_MAX_TOKENS_DEFAULT: u64 = 400;

/// Orders a brief's members by the seq of the `review.ratify` that admitted
/// each, then by `artifact_id`, so a brief reads in the order its members
/// were agreed (ADR-0046 d8). `position` names a row's (seq, id).
pub fn brief_order<T>(rows: &mut [T], position: impl Fn(&T) -> (u64, Uuid)) {
    rows.sort_by_key(|r| position(r));
}

/// The budget cut over the members' counts, in [`brief_order`]: the served
/// members are the longest prefix that fits `max_tokens`, and the first
/// that does not fit ends the brief, with no skip-ahead (ADR-0046 d8).
#[must_use]
pub fn brief_cut(member_tokens: &[u64], max_tokens: u64) -> Budget {
    cut(&[], member_tokens, Some(max_tokens)).expect("a budget is always sent")
}
