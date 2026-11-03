//! Deterministic term suggest: a query and a vocabulary release's labels,
//! NFC-normalized, matched by label prefix and by token, ranked in one fixed
//! total order and capped — matching, never interpretation, and no model
//! (ADR-0021 d9, d10; http-api.md, the vocabulary read surface; openapi.yaml
//! `suggestTerms`, `VocabSuggestion`).

use std::collections::BTreeMap;

use oml_domain::nfc;
use oml_domain::vocab::{Curie, Successors, TermStatus};
use oml_domain::vocab_edges::TipState;
use oml_domain::vocab_suggest::{ReleaseTerm, SuggestMatch, Suggestion, suggest};
use proptest::prelude::*;
use unicode_normalization::UnicodeNormalization;

fn curie(text: &str) -> Curie {
    Curie::parse(text).expect("a well-formed CURIE")
}

fn release(terms: &[(&str, &str, TipState)]) -> BTreeMap<Curie, ReleaseTerm> {
    terms
        .iter()
        .map(|(c, label, state)| {
            (
                curie(c),
                ReleaseTerm {
                    label: (*label).to_owned(),
                    state: state.clone(),
                },
            )
        })
        .collect()
}

fn active<'a>(c: &'a str, label: &'a str) -> (&'a str, &'a str, TipState) {
    (c, label, TipState::Active)
}

fn curies(suggestions: &[Suggestion]) -> Vec<&str> {
    suggestions.iter().map(|s| s.curie.as_str()).collect()
}

#[test]
fn an_exact_label_ranks_before_a_label_prefix_and_a_label_prefix_before_a_token() {
    let terms = release(&[
        active("biz:price-quote", "price quote"),
        active("biz:quote-request", "quote request"),
        active("biz:quote", "quote"),
        active("biz:invoice", "invoice"),
    ]);

    let suggested = suggest("quote", &terms, 50);

    // "invoice" starts nowhere with the query and is not suggested.
    assert_eq!(
        curies(&suggested),
        ["biz:quote", "biz:quote-request", "biz:price-quote"]
    );
    let matched: Vec<SuggestMatch> = suggested.iter().map(|s| s.matched).collect();
    assert_eq!(
        matched,
        [
            SuggestMatch::Exact,
            SuggestMatch::Prefix,
            SuggestMatch::Token
        ]
    );
}

#[test]
fn a_tie_in_match_is_broken_by_the_shorter_label_in_characters_then_by_curie() {
    let terms = release(&[
        active("biz:z-price", "price"),
        active("biz:a-prize", "prize"),
        active("biz:prim", "prim"),
    ]);
    assert_eq!(
        curies(&suggest("pri", &terms, 50)),
        ["biz:prim", "biz:a-prize", "biz:z-price"]
    );

    // "pré" is three characters in four UTF-8 bytes, "prex" four in four:
    // the length counted is characters of the NFC label, so a decomposed
    // "pre\u{301}" counts three as well.
    let terms = release(&[active("biz:a", "prex"), active("biz:z", "pre\u{301}")]);
    assert_eq!(curies(&suggest("pr", &terms, 50)), ["biz:z", "biz:a"]);
}

#[test]
fn the_query_and_the_labels_are_matched_after_nfc_normalization() {
    let decomposed = release(&[active("food:cafe-au-lait", "cafe\u{301} au lait")]);
    let composed = release(&[active("food:cafe-au-lait", "caf\u{e9} au lait")]);

    for terms in [&decomposed, &composed] {
        for query in ["caf\u{e9}", "cafe\u{301}"] {
            let suggested = suggest(query, terms, 50);
            assert_eq!(curies(&suggested), ["food:cafe-au-lait"], "{query:?}");
            assert_eq!(suggested[0].matched, SuggestMatch::Prefix);
        }
        assert_eq!(suggest("lait", terms, 50)[0].matched, SuggestMatch::Token);
        // "cafe" is a byte prefix of the decomposed spelling only; after
        // NFC it is a prefix of neither, so neither spelling suggests it.
        assert!(suggest("cafe", terms, 50).is_empty());
    }
    // The suggestion carries the label as the release holds it.
    assert_eq!(
        suggest("lait", &decomposed, 50)[0].label,
        "cafe\u{301} au lait"
    );
}

#[test]
fn suggest_folds_no_case() {
    let terms = release(&[active("biz:price-quote", "Price quote")]);
    assert!(suggest("price", &terms, 50).is_empty());
    assert!(suggest("QUOTE", &terms, 50).is_empty());
    assert_eq!(curies(&suggest("Price", &terms, 50)), ["biz:price-quote"]);
}

#[test]
fn a_deprecated_term_is_suggested_with_its_successors_and_ranked_like_an_active_one() {
    // openapi.yaml's `suggestTerms` example: an active and a deprecated term.
    let successor = Successors::ReplacedBy(curie("proc:price-quote"));
    let terms = release(&[
        ("proc:price-quote", "price quote", TipState::Active),
        (
            "proc:supplier-quote",
            "supplier quote",
            TipState::Deprecated(successor.clone()),
        ),
    ]);

    let suggested = suggest("quote", &terms, 50);
    assert_eq!(
        curies(&suggested),
        ["proc:price-quote", "proc:supplier-quote"]
    );
    assert_eq!(suggested[0].state.status(), TermStatus::Active);
    assert_eq!(suggested[1].state.status(), TermStatus::Deprecated);
    assert_eq!(suggested[1].state, TipState::Deprecated(successor));

    // Status ranks nothing: a deprecated exact match outranks an active
    // token match.
    let consider = Successors::Consider(vec![curie("proc:quote"), curie("proc:bid")]);
    let terms = release(&[
        (
            "proc:supplier-quote",
            "quote",
            TipState::Deprecated(consider.clone()),
        ),
        ("proc:price-quote", "price quote", TipState::Active),
    ]);
    let suggested = suggest("quote", &terms, 50);
    assert_eq!(
        curies(&suggested),
        ["proc:supplier-quote", "proc:price-quote"]
    );
    assert_eq!(suggested[0].state, TipState::Deprecated(consider));
}

// The properties below hold the implementation against an oracle written
// independently: matching by whitespace tails, ranking by an explicit key.

/// Characters a generated label or query is spelled from: case, whitespace,
/// and é in both its composed and its decomposed spelling.
const PIECES: [&str; 7] = ["a", "b", "A", " ", "e", "\u{e9}", "e\u{301}"];

fn text(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES.to_vec()), 0..max)
        .prop_map(|pieces| pieces.concat())
}

fn any_curie() -> impl Strategy<Value = Curie> {
    "[a-h]{1,2}".prop_map(|local| curie(&format!("t:{local}")))
}

fn any_state() -> impl Strategy<Value = TipState> {
    prop_oneof![
        Just(TipState::Active),
        any_curie().prop_map(|c| TipState::Deprecated(Successors::ReplacedBy(c))),
        prop::collection::vec(any_curie(), 1..3)
            .prop_map(|cs| TipState::Deprecated(Successors::Consider(cs))),
    ]
}

fn any_release() -> impl Strategy<Value = BTreeMap<Curie, ReleaseTerm>> {
    prop::collection::btree_map(
        any_curie(),
        (text(7), any_state()).prop_map(|(label, state)| ReleaseTerm { label, state }),
        0..48,
    )
}

fn oracle_match(query: &str, label: &str) -> Option<SuggestMatch> {
    let (query, label) = (nfc(query), nfc(label));
    if label == query {
        return Some(SuggestMatch::Exact);
    }
    if label.starts_with(&query) {
        return Some(SuggestMatch::Prefix);
    }
    let token = label
        .match_indices(char::is_whitespace)
        .map(|(at, space)| label[at + space.len()..].trim_start())
        .any(|tail| !tail.is_empty() && tail.starts_with(&query));
    token.then_some(SuggestMatch::Token)
}

fn key(s: &Suggestion) -> (u8, usize, String) {
    let rank = match s.matched {
        SuggestMatch::Exact => 0,
        SuggestMatch::Prefix => 1,
        SuggestMatch::Token => 2,
    };
    (
        rank,
        nfc(&s.label).chars().count(),
        s.curie.as_str().to_owned(),
    )
}

/// Every matching term of the release, in the documented order, uncapped.
fn oracle(query: &str, terms: &BTreeMap<Curie, ReleaseTerm>) -> Vec<Suggestion> {
    let mut matched: Vec<Suggestion> = terms
        .iter()
        .filter_map(|(c, t)| {
            oracle_match(query, &t.label).map(|matched| Suggestion {
                curie: c.clone(),
                label: t.label.clone(),
                state: t.state.clone(),
                matched,
            })
        })
        .collect();
    matched.sort_by_key(key);
    matched
}

proptest! {
    #[test]
    fn the_same_query_and_release_give_the_same_suggestions(
        query in text(4),
        terms in any_release(),
        cap in 0usize..16,
    ) {
        let first = suggest(&query, &terms, cap);
        prop_assert_eq!(&first, &suggest(&query, &terms.clone(), cap));
        // The release built in the opposite order is the same release.
        let reversed: BTreeMap<Curie, ReleaseTerm> =
            terms.clone().into_iter().rev().collect();
        prop_assert_eq!(&first, &suggest(&query, &reversed, cap));
    }

    #[test]
    fn canonically_equivalent_queries_give_the_same_suggestions(
        query in text(4),
        terms in any_release(),
    ) {
        let decomposed: String = query.nfd().collect();
        let composed: String = query.nfc().collect();
        prop_assert_eq!(suggest(&decomposed, &terms, 50), suggest(&composed, &terms, 50));
    }

    #[test]
    fn the_ranking_is_a_strict_total_order_by_match_then_label_length_then_curie(
        query in text(4),
        terms in any_release(),
    ) {
        let suggested = suggest(&query, &terms, usize::MAX);
        // Every matching term, and only those, in the documented order.
        prop_assert_eq!(&suggested, &oracle(&query, &terms));
        // No two suggestions tie: the order is strict.
        for pair in suggested.windows(2) {
            prop_assert!(key(&pair[0]) < key(&pair[1]), "{:?}", pair);
        }
    }

    #[test]
    fn suggest_returns_at_most_cap_suggestions_and_a_smaller_cap_is_a_prefix_of_a_larger(
        query in text(4),
        terms in any_release(),
        cap in 0usize..16,
    ) {
        let all = oracle(&query, &terms);
        let capped = suggest(&query, &terms, cap);
        prop_assert_eq!(capped.len(), cap.min(all.len()));
        prop_assert_eq!(&capped[..], &all[..capped.len()]);
    }

    #[test]
    fn every_suggestion_is_an_unchanged_term_of_the_release_and_no_matching_deprecated_term_is_dropped(
        query in text(4),
        terms in any_release(),
    ) {
        for s in suggest(&query, &terms, usize::MAX) {
            let term = terms.get(&s.curie);
            prop_assert!(term.is_some(), "{} is not in the release", s.curie);
            let term = term.expect("checked");
            prop_assert_eq!(&s.label, &term.label);
            prop_assert_eq!(&s.state, &term.state);
        }
        // A deprecated term that matches is never dropped.
        let deprecated_matches = terms
            .values()
            .filter(|t| t.state.status() == TermStatus::Deprecated)
            .filter(|t| oracle_match(&query, &t.label).is_some())
            .count();
        let deprecated_suggested = suggest(&query, &terms, usize::MAX)
            .iter()
            .filter(|s| s.state.status() == TermStatus::Deprecated)
            .count();
        prop_assert_eq!(deprecated_suggested, deprecated_matches);
    }
}
