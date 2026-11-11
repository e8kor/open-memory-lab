//! The red-team corpus (ADR-0074 d15): every sample states what it expects,
//! and the report under the compiled rule set is the committed baseline, so
//! a rule change that moves any sample's verdict or any rate changes
//! `tests/corpus/baseline.txt` in the same change, where review sees it.

#[path = "corpus/harness.rs"]
mod harness;

use std::collections::BTreeSet;

#[test]
fn every_corpus_sample_states_its_expected_verdict() {
    let samples = harness::load();
    assert!(!samples.is_empty(), "the corpus is not empty");
    let ids: BTreeSet<&str> = samples.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids.len(), samples.len(), "every sample id is unique");
    for s in &samples {
        let at = format!("{} {}", s.file, s.id);
        assert!(!s.id.is_empty(), "{at}: an id");
        assert!(
            harness::VERDICTS.contains(&s.expected.as_str()),
            "{at}: an expected verdict among {:?}, not {:?}",
            harness::VERDICTS,
            s.expected
        );
        assert!(
            harness::SETS.contains(&s.set.as_str()),
            "{at}: a set among {:?}",
            harness::SETS
        );
        assert!(!s.class.is_empty(), "{at}: a class");
        assert!(!s.lang.is_empty(), "{at}: a language");
        for field in ["source", "spdx", "date"] {
            assert!(
                s.raw[field].as_str().is_some_and(|v| !v.is_empty()),
                "{at}: its {field}"
            );
        }
        assert!(
            [
                "Apache-2.0",
                "MIT",
                "BSD-2-Clause",
                "BSD-3-Clause",
                "CC-BY-4.0",
                "CC0-1.0"
            ]
            .contains(&s.raw["spdx"].as_str().unwrap_or_default()),
            "{at}: a licence d15 admits"
        );
        assert!(!s.text.is_empty(), "{at}: a text that decodes");
        // Live injection text and secrets never sit in the tree as plain
        // text: a sample that should not pass is stored encoded.
        if s.malicious() && s.set != "twin" {
            assert!(
                s.raw.get("text").is_none(),
                "{at}: a malicious sample is stored base64-encoded"
            );
        }
        if s.class.starts_with("secret.") && s.set == "malicious" {
            assert_eq!(
                s.raw["parts_b64"].as_array().map(Vec::len),
                Some(2),
                "{at}: a secret sample is stored in two halves"
            );
        }
        if s.set == "twin" {
            let of = s.twin_of.as_deref().unwrap_or_default();
            assert!(ids.contains(of), "{at}: its twin_of {of:?} is a sample");
        }
        if s.set == "heldout" {
            assert!(
                s.raw["commit"].as_str().is_some_and(|c| c.len() == 40),
                "{at}: a held-out sample names the commit it was frozen from"
            );
        }
    }
}

#[test]
fn the_redteam_report_matches_the_committed_baseline() {
    let report = harness::report(&harness::load(), true);
    let path = harness::corpus_dir().join("baseline.txt");
    if std::env::var_os("OML_REDTEAM_BLESS").is_some() {
        std::fs::write(&path, &report).expect("write the baseline");
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    if committed != report {
        let first = committed
            .lines()
            .zip(report.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(committed.lines().count().min(report.lines().count()));
        panic!(
            "the redteam report differs from tests/corpus/baseline.txt from line {}: \
             rerun with OML_REDTEAM_BLESS=1 and commit the baseline with the rule change\n\
             committed: {:?}\nnow:       {:?}",
            first + 1,
            committed.lines().nth(first),
            report.lines().nth(first)
        );
    }
}
