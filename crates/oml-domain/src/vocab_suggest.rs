//! Term suggest: the deterministic prefix+label typeahead over one
//! vocabulary release — a query and the release's term labels, normalized
//! to NFC, matched by label prefix and by token, ranked in one fixed total
//! order and capped. It is matching, never interpretation: no model runs
//! and nothing outside the given release is read (ADR-0021 d9, d10).
//!
//! The surface is declared on `docs/api/http-api.md` (the vocabulary read
//! surface, `GET /v2/vocab/suggest`) and `docs/api/openapi.yaml`
//! (`suggestTerms`, `VocabSuggestion`); the normalization is admission's
//! NFC step (`docs/architecture/admission.md`). This module mirrors them
//! and adds nothing.
//!
//! Suggest refuses nothing: a query that matches no label answers an empty
//! list, which is also how a vocabulary outside the caller's read scope
//! answers, since the caller passes only the terms it may read (http-api.md:
//! the read surface fails closed as absence, "an empty match elsewhere").

use std::collections::BTreeMap;

use crate::model::nfc;
use crate::vocab::Curie;
use crate::vocab_edges::TipState;

/// One term of a vocabulary release, as suggest reads it: the label a
/// suggestion shows and the term's served tip state — active, or deprecated
/// with the successors its deprecating submission declared (ADR-0021 d3;
/// openapi.yaml `VocabSuggestion`).
// reading: the docs give a suggestion a `label` but name no field of a term
// it is read from; the caller passes the label it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTerm {
    pub label: String,
    pub state: TipState,
}

/// How a label matched the query, best first: the declaration order is the
/// ranking order.
// reading: the docs name no tokenizer; a token ends at Unicode White_Space
// (`char::is_whitespace`) and nowhere else, so `churn-risk` is one token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SuggestMatch {
    /// The label is the query.
    Exact,
    /// The label starts with the query.
    Prefix,
    /// The label, from the start of one of its later tokens, starts with
    /// the query. A token is a maximal run of characters that are not
    /// whitespace.
    Token,
}

/// One suggestion: a term of the release, unchanged, and how it matched.
/// `state.status()` is the suggestion's `term_status`; a deprecated term
/// carries its successors (openapi.yaml `VocabSuggestion`; ADR-0021 d3, d9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub curie: Curie,
    pub label: String,
    pub state: TipState,
    pub matched: SuggestMatch,
}

/// The terms of `release` whose label matches `query`, best first, at most
/// `cap` of them (ADR-0021 d9; http-api.md `GET /v2/vocab/suggest`).
///
/// The query and every label are normalized to NFC before matching, so two
/// canonically equivalent spellings match alike (admission.md). The order is
/// total: by [`SuggestMatch`], then by the shorter label in characters of
/// its NFC form, then by CURIE in code-point order — and a release holds one
/// term per CURIE, so no two suggestions tie. A deprecated term is
/// suggested and ranked like an active one.
// reading: the docs name NFC as the one normalization (admission.md's
// capture step; retrieval.md's recall query) and name no case or accent
// folding for suggest, so suggest folds neither: `Price` does not match
// `price`.
// reading: "prefix+label typeahead" is read as prefix matching over labels;
// the CURIE itself is not matched.
// reading: the docs call suggest deterministic but name no order; the order
// here is the best match first, then the shortest label, then the CURIE,
// which makes it total.
// reading: openapi.yaml's `suggestTerms` example lists a deprecated term
// among the suggestions with its `term_status`, and the docs rank nothing
// by status, so a deprecated term is neither dropped nor demoted in rank.
#[must_use]
pub fn suggest(query: &str, release: &BTreeMap<Curie, ReleaseTerm>, cap: usize) -> Vec<Suggestion> {
    let query = nfc(query);
    let mut ranked: Vec<(SuggestMatch, usize, &Curie, &ReleaseTerm)> = release
        .iter()
        .filter_map(|(curie, term)| {
            let label = nfc(&term.label);
            label_match(&query, &label).map(|m| (m, label.chars().count(), curie, term))
        })
        .collect();
    ranked.sort_unstable_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    ranked
        .into_iter()
        .take(cap)
        .map(|(matched, _, curie, term)| Suggestion {
            curie: curie.clone(),
            label: term.label.clone(),
            state: term.state.clone(),
            matched,
        })
        .collect()
}

/// How the NFC `label` matches the NFC `query`, if it does.
fn label_match(query: &str, label: &str) -> Option<SuggestMatch> {
    if label == query {
        return Some(SuggestMatch::Exact);
    }
    if label.starts_with(query) {
        return Some(SuggestMatch::Prefix);
    }
    let mut after_space = false;
    let token = label.char_indices().any(|(at, c)| {
        let token_start = after_space && !c.is_whitespace();
        after_space = c.is_whitespace();
        token_start && label[at..].starts_with(query)
    });
    token.then_some(SuggestMatch::Token)
}
