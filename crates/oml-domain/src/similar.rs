//! The near-copy check's pure core (ADR-0050 d1, d17): a text's words, its
//! `same_text` key and MinHash band keys, the guard that keeps short texts
//! out of the near-copy relation, and the exact relation the core verifies
//! before any row is named.
//!
//! No model and no recall: a write is compared with the rows it may read by
//! integer keys an index serves under row security, then each candidate by
//! the exact Jaccard similarity of the two word sets. The keys equal
//! migration 0027's `oml_similar_keys`, which a law holds (d17).
//!
//! Adopted from Graphiti's deterministic resolution (`dedup_helpers.py`,
//! Apache-2.0), re-implemented: whole memories instead of entity names, so
//! word sets instead of character 3-grams, 60 permutations in 10 bands of 6
//! instead of 32 in 8 of 4, and an entropy floor set for sentences.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

/// The Jaccard similarity at and above which two texts' word sets are a
/// near copy (d1).
pub const NEAR_COPY_JACCARD: f64 = 0.9;

/// The fewest distinct words a text needs to be a near copy of another:
/// below it a ratio of 0.9 needs the same word set, which for two different
/// texts is the same words reordered (d1).
pub const MIN_DISTINCT_WORDS: usize = 10;

/// The least Shannon entropy, in bits per character, of a text's word
/// characters for it to be a near copy: under it the text is a repetition
/// (d1). The labelled set's lowest is 3.52.
pub const MIN_ENTROPY_BITS: f64 = 3.0;

/// MinHash permutations (d17).
pub const PERMUTATIONS: usize = 60;

/// Bands of the signature, each [`ROWS`] minima (d17).
pub const BANDS: usize = 10;

/// Minima per band (d17).
pub const ROWS: usize = 6;

/// The most candidates the core verifies per write, those sharing the most
/// bands first (d17).
pub const MAX_CANDIDATES: usize = 32;

/// The most rows a write's answer names (d1).
pub const MAX_LISTED: usize = 3;

/// The band of the `same_text` key; the MinHash bands are `1..=BANDS`.
pub const SAME_TEXT_BAND: i16 = 0;

/// The Mersenne prime 2³¹ − 1, the field the permutations map into.
const PRIME: i64 = 2_147_483_647;

/// A string's 64-bit id: SHA-256's first eight bytes, big-endian, signed;
/// ADR-0052 d2's `oml_token_id`, which SQL computes equally.
#[must_use]
pub fn token_id(s: &str) -> i64 {
    let digest = Sha256::digest(s.as_bytes());
    let mut first = [0_u8; 8];
    first.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(first)
}

/// The text's words, in order: ASCII letters lower-cased, split at every
/// ASCII character that is not a letter or a digit; a character outside
/// ASCII belongs to its word (d17). The caller passes NFC text.
#[must_use]
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_ascii() && !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c.to_ascii_lowercase());
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// The Shannon entropy, in bits per character, of the words' characters.
#[must_use]
pub fn entropy_bits(words: &[String]) -> f64 {
    let mut counts = std::collections::BTreeMap::<char, u32>::new();
    let mut total = 0_u32;
    for c in words.iter().flat_map(|w| w.chars()) {
        *counts.entry(c).or_default() += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    let total = f64::from(total);
    counts
        .values()
        .map(|&n| {
            let p = f64::from(n) / total;
            -p * p.log2()
        })
        .sum()
}

/// A text as the check compares it: its words in order and their set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The words, in order (`same_text`).
    pub words: Vec<String>,
    /// The distinct words (`near_copy`).
    pub set: BTreeSet<String>,
}

impl Fingerprint {
    /// The text's fingerprint.
    #[must_use]
    pub fn of(text: &str) -> Self {
        let words = words(text);
        let set = words.iter().cloned().collect();
        Self { words, set }
    }

    /// Whether the text may be a near copy at all: at least
    /// [`MIN_DISTINCT_WORDS`] distinct words and [`MIN_ENTROPY_BITS`] of
    /// entropy (d1). A text that may not is compared for `same_text` alone.
    #[must_use]
    pub fn may_be_near_copy(&self) -> bool {
        self.set.len() >= MIN_DISTINCT_WORDS && entropy_bits(&self.words) >= MIN_ENTROPY_BITS
    }

    /// The `same_text` key, or `None` for a text with no word.
    #[must_use]
    pub fn same_text_key(&self) -> Option<i64> {
        (!self.words.is_empty()).then(|| token_id(&format!("x:{}", self.words.join(" "))))
    }

    /// The MinHash signature: per permutation, the least image of a word's
    /// id; empty for a text with no word.
    #[must_use]
    pub fn signature(&self) -> Vec<i64> {
        if self.set.is_empty() {
            return Vec::new();
        }
        let xs: Vec<i64> = self
            .set
            .iter()
            .map(|w| token_id(w).rem_euclid(PRIME))
            .collect();
        (0..PERMUTATIONS)
            .map(|i| {
                let (a, b) = permutation(i);
                xs.iter().map(|&x| (a * x + b) % PRIME).min().unwrap_or(0)
            })
            .collect()
    }

    /// The band keys, `(band, key)` for bands `1..=BANDS`; empty for a text
    /// with no word.
    #[must_use]
    pub fn band_keys(&self) -> Vec<(i16, i64)> {
        let signature = self.signature();
        signature
            .chunks(ROWS)
            .enumerate()
            .map(|(j, minima)| {
                let band = i16::try_from(j + 1).unwrap_or(i16::MAX);
                let joined: Vec<String> = minima.iter().map(ToString::to_string).collect();
                (band, token_id(&format!("b{band}:{}", joined.join(","))))
            })
            .collect()
    }

    /// Every key a stored version holds: its `same_text` key (band 0) and
    /// its band keys, whatever its length; the guard applies when a write
    /// is compared (d17).
    #[must_use]
    pub fn stored_keys(&self) -> Vec<(i16, i64)> {
        let mut keys: Vec<(i16, i64)> = self
            .same_text_key()
            .map(|k| (SAME_TEXT_BAND, k))
            .into_iter()
            .collect();
        keys.extend(self.band_keys());
        keys
    }

    /// The keys a write looks up: its `same_text` key, and its band keys
    /// only when it may be a near copy (d1).
    #[must_use]
    pub fn query_keys(&self) -> Vec<(i16, i64)> {
        if self.may_be_near_copy() {
            self.stored_keys()
        } else {
            self.same_text_key()
                .map(|k| (SAME_TEXT_BAND, k))
                .into_iter()
                .collect()
        }
    }
}

/// Permutation `i`'s coefficients: `a` in `1..PRIME`, `b` in `0..PRIME`,
/// from the ids of `minhash:a:i` and `minhash:b:i` (d17).
fn permutation(i: usize) -> (i64, i64) {
    let a = token_id(&format!("minhash:a:{i}")).rem_euclid(PRIME - 1) + 1;
    let b = token_id(&format!("minhash:b:{i}")).rem_euclid(PRIME);
    (a, b)
}

/// The Jaccard similarity of two word sets; 0 when both are empty.
#[must_use]
pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    #[allow(clippy::cast_precision_loss, reason = "word counts are small")]
    let ratio = shared as f64 / union as f64;
    ratio
}

/// How a held or served row relates to a write (d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relation {
    /// The same words in the same order.
    SameText,
    /// Not the same text; word sets at Jaccard ≥ [`NEAR_COPY_JACCARD`], both
    /// texts past the guard.
    NearCopy,
}

impl Relation {
    /// The name the answer, the refusal and the metric use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SameText => "same_text",
            Self::NearCopy => "near_copy",
        }
    }
}

/// How `row` relates to the write `new`, with the Jaccard similarity of
/// their word sets (1.0 for `same_text`), or `None` when it is neither:
/// the exact verification a candidate passes before it is named (d1, d17).
#[must_use]
pub fn relate(new: &Fingerprint, row: &Fingerprint) -> Option<(Relation, f64)> {
    if new.words.is_empty() {
        return None;
    }
    if new.words == row.words {
        return Some((Relation::SameText, 1.0));
    }
    if !new.may_be_near_copy() || !row.may_be_near_copy() {
        return None;
    }
    let j = jaccard(&new.set, &row.set);
    (j >= NEAR_COPY_JACCARD).then_some((Relation::NearCopy, j))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "Production deploys run only on Tuesday and Thursday between 10:00 and \
                        12:00 CET; outside that window a deploy needs the on-call lead's approval.";

    #[test]
    fn words_split_at_ascii_punctuation_and_keep_other_scripts_whole() {
        assert_eq!(
            words("Use `pnpm`, not NPM: v0.5.7 — ok?"),
            vec!["use", "pnpm", "not", "npm", "v0", "5", "7", "—", "ok"]
        );
        assert_eq!(words("Grüße, Straße"), vec!["grüße", "straße"]);
        assert!(words(" ... ").is_empty());
    }

    #[test]
    fn same_text_ignores_case_punctuation_and_spacing_and_keeps_order() {
        let a = Fingerprint::of(LONG);
        let b = Fingerprint::of(&format!("  {}  ", LONG.to_lowercase().replace(';', ",")));
        assert_eq!(relate(&a, &b), Some((Relation::SameText, 1.0)));
        assert_eq!(a.same_text_key(), b.same_text_key());
        let reordered = Fingerprint::of("Port 8080, not 8081.");
        let swapped = Fingerprint::of("Port 8081, not 8080.");
        assert_ne!(reordered.same_text_key(), swapped.same_text_key());
    }

    #[test]
    fn a_short_text_is_never_a_near_copy() {
        for (a, b) in [
            ("Port 8080, not 8081.", "Port 8081, not 8080."),
            ("Use port 8080.", "Use port 8081."),
            ("Deploy on Tuesday.", "Deploy on Thursday."),
            (
                "yes yes yes yes yes yes yes yes yes yes yes",
                "yes yes yes yes yes yes yes yes yes yes",
            ),
        ] {
            assert_eq!(
                relate(&Fingerprint::of(a), &Fingerprint::of(b)),
                None,
                "{a} / {b}"
            );
        }
        let short = Fingerprint::of("Port 8080, not 8081.");
        assert_eq!(
            short.query_keys().len(),
            1,
            "a short text looks up same_text alone"
        );
    }

    #[test]
    fn a_one_word_change_in_a_long_text_is_a_near_copy_and_a_rewrite_is_not() {
        let a = Fingerprint::of(LONG);
        let typo = Fingerprint::of(&LONG.replace("approval", "aproval"));
        let (relation, j) = relate(&a, &Fingerprint::of(&format!("Note: {LONG}"))).unwrap();
        assert_eq!(relation, Relation::NearCopy);
        assert!((NEAR_COPY_JACCARD..1.0).contains(&j), "{j}");
        assert_eq!(relate(&a, &typo).map(|r| r.0), Some(Relation::NearCopy));
        let other = Fingerprint::of(
            "Deploys to production need the on-call lead to approve them unless they land \
             on a Tuesday or a Thursday morning in the agreed window.",
        );
        assert_eq!(relate(&a, &other), None);
    }

    #[test]
    fn a_near_copy_shares_a_band_with_its_original() {
        let a = Fingerprint::of(LONG);
        let b = Fingerprint::of(&format!("Note: {LONG}"));
        let shared = a
            .band_keys()
            .iter()
            .filter(|k| b.band_keys().contains(k))
            .count();
        assert!(shared >= 1, "{shared}");
        assert_eq!(a.band_keys().len(), BANDS);
        assert_eq!(a.signature().len(), PERMUTATIONS);
    }

    #[test]
    fn keys_are_deterministic_and_empty_for_a_text_with_no_word() {
        assert_eq!(
            Fingerprint::of(LONG).stored_keys(),
            Fingerprint::of(LONG).stored_keys()
        );
        assert_eq!(Fingerprint::of(LONG).stored_keys().len(), BANDS + 1);
        assert!(Fingerprint::of("--- !!!").stored_keys().is_empty());
        assert_eq!(relate(&Fingerprint::of(""), &Fingerprint::of("")), None);
    }

    #[test]
    fn token_id_is_adr_0052s() {
        // SHA-256("postgres")'s first eight bytes, big-endian.
        assert_eq!(
            token_id("postgres"),
            i64::from_be_bytes([0xa9, 0x42, 0xb3, 0x7c, 0xcf, 0xaf, 0x5a, 0x81])
        );
    }
}
