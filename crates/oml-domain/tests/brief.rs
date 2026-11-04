//! A workarea's brief: its members in ratification order, cut by a token
//! budget under recall's prefix rule (ADR-0046 d7, d8).

use oml_domain::brief::{BRIEF_LABEL, BRIEF_MAX_TOKENS_DEFAULT, brief_cut, brief_order};
use uuid::Uuid;

#[test]
fn the_brief_edge_label_is_brief_and_the_default_budget_is_400_tokens() {
    assert_eq!(BRIEF_LABEL, "brief");
    assert_eq!(BRIEF_MAX_TOKENS_DEFAULT, 400);
}

#[test]
fn members_are_ordered_by_ratification_seq_then_artifact_id() {
    let (a, b, c) = (Uuid::from_u128(3), Uuid::from_u128(1), Uuid::from_u128(2));
    let mut rows = vec![(9_u64, a), (4, b), (9, c), (4, a)];
    brief_order(&mut rows, |r| (r.0, r.1));
    assert_eq!(rows, vec![(4, b), (4, a), (9, c), (9, a)]);
}

#[test]
fn the_first_member_that_does_not_fit_the_budget_ends_the_brief() {
    let cut = brief_cut(&[100, 250, 10, 30], 360);
    assert_eq!(cut.hits_served, 3);
    assert_eq!(cut.used_tokens, 360);
    assert!(cut.truncated);
    assert_eq!(cut.hits_omitted, 1);
    // No skip-ahead: a later member that would fit is not served.
    let cut = brief_cut(&[100, 400, 10], 300);
    assert_eq!(cut.hits_served, 1);
    let empty = brief_cut(&[], 400);
    assert_eq!((empty.hits_served, empty.truncated), (0, false));
}
