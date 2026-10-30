//! Budget tokens, the budget cut, steering order and hit order
//! (ADR-0025 d6, d7, d8; retrieval.md, "Token budgets" and "The
//! recall-ordering contract").

use oml_domain::EventTime;
use oml_domain::budget::{Budget, COUNTER_ID, budget_tokens, cut, hit_order, steering_order};
use proptest::prelude::*;
use uuid::Uuid;

#[test]
fn budget_tokens_is_the_ceiling_of_bytes_over_four() {
    assert_eq!(COUNTER_ID, "oml-tc/1");
    assert_eq!(budget_tokens(0), 0, "an empty body counts nothing");
    assert_eq!(budget_tokens(1), 1);
    assert_eq!(budget_tokens(4), 1);
    assert_eq!(budget_tokens(5), 2);
    // The unit is UTF-8 octets, never characters: one four-octet character
    // is one budget token, and three three-octet characters are three.
    let clef = "\u{1D11E}";
    assert_eq!(clef.chars().count(), 1);
    assert_eq!(budget_tokens(clef.len() as u64), 1);
    let nihongo = "\u{65E5}\u{672C}\u{8A9E}";
    assert_eq!(nihongo.len(), 9);
    assert_eq!(budget_tokens(nihongo.len() as u64), 3);
}

#[test]
fn steering_is_never_cut_even_past_the_budget() {
    let budget = cut(&[400, 300], &[50, 10], Some(500)).expect("a budget was sent");
    assert_eq!(
        budget,
        Budget {
            max_tokens: 500,
            steering_tokens: 700,
            used_tokens: 700,
            truncated: true,
            hits_served: 0,
            hits_omitted: 2,
            next_hit_tokens: Some(50),
        }
    );
}

#[test]
fn hits_served_are_the_longest_rank_prefix_that_fits() {
    // remaining = 100 − 10 = 90; 30 + 40 = 70 fits, + 20 = 90 fits, + 1 does not.
    let budget = cut(&[10], &[30, 40, 20, 1], Some(100)).expect("a budget was sent");
    assert_eq!(budget.hits_served, 3);
    assert_eq!(budget.used_tokens, 100);
    assert!(budget.truncated);
    assert_eq!(budget.hits_omitted, 1);
    assert_eq!(budget.next_hit_tokens, Some(1));

    // Everything fits: nothing is cut and next_hit_tokens is null.
    let budget = cut(&[10], &[30, 40], Some(100)).expect("a budget was sent");
    assert_eq!(
        budget,
        Budget {
            max_tokens: 100,
            steering_tokens: 10,
            used_tokens: 80,
            truncated: false,
            hits_served: 2,
            hits_omitted: 0,
            next_hit_tokens: None,
        }
    );
}

#[test]
fn a_hit_that_does_not_fit_ends_the_list_even_when_a_later_one_would_fit() {
    // The worked example of retrieval.md, "Token budgets".
    let budget = cut(&[120], &[300, 250, 90], Some(500)).expect("a budget was sent");
    assert_eq!(
        budget,
        Budget {
            max_tokens: 500,
            steering_tokens: 120,
            used_tokens: 420,
            truncated: true,
            hits_served: 1,
            hits_omitted: 2,
            next_hit_tokens: Some(250),
        }
    );

    // A top hit larger than what remains serves zero hits.
    let budget = cut(&[], &[600, 1], Some(500)).expect("a budget was sent");
    assert_eq!(budget.hits_served, 0);
    assert_eq!(budget.used_tokens, 0);
    assert_eq!(budget.next_hit_tokens, Some(600));
}

#[test]
fn no_budget_returns_no_budget_object() {
    assert_eq!(cut(&[120], &[300, 250, 90], None), None);
    assert_eq!(cut(&[], &[], None), None);
}

#[test]
fn used_exceeds_max_only_through_steering() {
    // Steering alone past the budget: used = steering, and only hits of
    // 0 budget tokens (empty bodies) can follow it.
    let budget = cut(&[30, 30], &[0, 0, 5, 0], Some(50)).expect("a budget was sent");
    assert_eq!(budget.steering_tokens, 60);
    assert_eq!(budget.used_tokens, 60);
    assert_eq!(budget.hits_served, 2);
    assert_eq!(budget.next_hit_tokens, Some(5));

    // Steering within the budget: hits never push used past max.
    let budget = cut(&[30], &[20, 1], Some(50)).expect("a budget was sent");
    assert_eq!(budget.used_tokens, 50);
    assert!(budget.used_tokens <= budget.max_tokens);
    assert_eq!(budget.hits_served, 1);
}

fn at(s: &str) -> EventTime {
    EventTime::parse(s).expect("fixed-width timestamp")
}

#[test]
fn steering_orders_by_depth_then_promoted_at_then_id() {
    struct Row {
        depth: u32,
        promoted_at: EventTime,
        artifact_id: Uuid,
    }
    let row = |depth, promoted_at: &str, id| Row {
        depth,
        promoted_at: at(promoted_at),
        artifact_id: Uuid::from_u128(id),
    };
    let mut rows = vec![
        // Deepest, earliest: depth wins over promoted_at.
        row(2, "2026-01-01T00:00:00.000000Z", 0x01),
        // Same depth and time: artifact_id ascending breaks the tie.
        row(1, "2026-03-01T00:00:00.000000Z", 0x0B),
        row(1, "2026-03-01T00:00:00.000000Z", 0x0A),
        // Same depth: the earlier promotion first, whatever the id.
        row(1, "2026-02-01T00:00:00.000000Z", 0xFF),
        // The universe root.
        row(0, "2026-12-31T23:59:59.999999Z", 0x02),
    ];
    steering_order(&mut rows, |r| (r.depth, &r.promoted_at, r.artifact_id));
    let order: Vec<u128> = rows.iter().map(|r| r.artifact_id.as_u128()).collect();
    assert_eq!(order, vec![0x02, 0xFF, 0x0A, 0x0B, 0x01]);
}

#[test]
fn hits_order_by_score_descending_then_artifact_id_ascending() {
    let mut rows = vec![
        (0.5_f32, Uuid::from_u128(0x03)),
        (0.9, Uuid::from_u128(0x02)),
        (0.5, Uuid::from_u128(0x01)),
        (0.1, Uuid::from_u128(0x00)),
    ];
    hit_order(&mut rows, |r| *r);
    let order: Vec<u128> = rows.iter().map(|r| r.1.as_u128()).collect();
    assert_eq!(order, vec![0x02, 0x01, 0x03, 0x00]);
}

/// Counts in the range a bounded claim body produces.
fn arb_counts() -> impl Strategy<Value = Vec<u64>> {
    proptest::collection::vec(0_u64..2_000, 0..12)
}

proptest! {
    #[test]
    fn the_cut_is_deterministic(
        steering in arb_counts(),
        hits in arb_counts(),
        max in 1_u64..20_000,
    ) {
        prop_assert_eq!(
            cut(&steering, &hits, Some(max)),
            cut(&steering, &hits, Some(max))
        );
    }

    #[test]
    fn the_served_hits_are_the_longest_prefix_within_what_steering_leaves(
        steering in arb_counts(),
        hits in arb_counts(),
        max in 1_u64..20_000,
    ) {
        let budget = cut(&steering, &hits, Some(max)).expect("a budget was sent");
        let s: u64 = steering.iter().sum();
        let remaining = max.saturating_sub(s);
        let j = budget.hits_served;
        let served: u64 = hits[..j].iter().sum();
        prop_assert_eq!(budget.max_tokens, max);
        prop_assert_eq!(budget.steering_tokens, s);
        prop_assert!(served <= remaining);
        // Longest: the next hit, if any, does not fit.
        if let Some(&next) = hits.get(j) {
            prop_assert!(served + next > remaining);
        }
        prop_assert_eq!(budget.used_tokens, s + served);
        prop_assert_eq!(budget.truncated, j < hits.len());
        prop_assert_eq!(budget.hits_omitted, hits.len() - j);
        prop_assert_eq!(budget.next_hit_tokens, hits.get(j).copied());
        prop_assert!(budget.used_tokens <= max || budget.used_tokens == s);
    }

    #[test]
    fn the_cut_is_monotone_in_max_tokens(
        steering in arb_counts(),
        hits in arb_counts(),
        max in 1_u64..20_000,
        more in 0_u64..20_000,
    ) {
        let smaller = cut(&steering, &hits, Some(max)).expect("a budget was sent");
        let larger = cut(&steering, &hits, Some(max + more)).expect("a budget was sent");
        prop_assert!(smaller.hits_served <= larger.hits_served);
        prop_assert!(smaller.used_tokens <= larger.used_tokens);
    }
}
