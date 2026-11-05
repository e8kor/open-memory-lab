//! The adapters of `oml sync` (ADR-0057 d7, d10, d21, d16 step 2), golden
//! against real ADRs copied into `tests/fixtures/sync/adr`: every decision
//! whole, rejected alternatives from a table or bullets, and the edges the
//! header states.

use std::path::{Path, PathBuf};

use oml_cli::sync::adapter::{Adapter, EdgeStatement, Extraction, Item, Kind, fit_key};
use oml_cli::sync::adr::AdrAdapter;
use oml_cli::sync::hash::md_hash;
use oml_cli::sync::headings::HeadingsAdapter;
use oml_cli::sync::lines::LinesAdapter;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync/adr")
}

fn fixture(prefix: &str) -> (String, Vec<u8>) {
    let dir = fixtures();
    let name = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .find(|n| n.starts_with(prefix))
        .unwrap_or_else(|| panic!("no fixture {prefix}"));
    let bytes = std::fs::read(dir.join(&name)).unwrap();
    (format!("docs/adr/{name}"), bytes)
}

fn adr(prefix: &str) -> Extraction {
    let (path, bytes) = fixture(prefix);
    AdrAdapter.extract(&path, &bytes).unwrap()
}

fn get<'a>(x: &'a Extraction, key: &str) -> &'a Item {
    x.items
        .iter()
        .find(|i| i.claim_key == key)
        .unwrap_or_else(|| panic!("no item {key}: {:?}", keys(x)))
}

fn keys(x: &Extraction) -> Vec<&str> {
    x.items.iter().map(|i| i.claim_key.as_str()).collect()
}

/// The decision handles under `## Decision`, counted without the adapter: a
/// line at column 0 that opens `- **d<digit>`.
fn decision_lines(doc: &str) -> Vec<String> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in doc.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            inside = h.trim().to_lowercase().starts_with("decision");
            continue;
        }
        if inside
            && let Some(rest) = line.strip_prefix("- **d")
            && rest.starts_with(|c: char| c.is_ascii_digit())
        {
            let handle: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            out.push(format!("d{handle}"));
        }
    }
    out
}

#[test]
fn extraction_is_a_pure_function_of_path_and_bytes() {
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        let name = format!("docs/adr/{}", path.file_name().unwrap().to_string_lossy());
        for adapter in [Kind::Adr, Kind::Headings, Kind::Lines] {
            let a = adapter.adapter();
            assert_eq!(
                a.extract(&name, &bytes),
                a.extract(&name, &bytes),
                "{name} under {}",
                a.name()
            );
        }
    }
}

#[test]
fn the_adr_adapter_yields_each_decision_a_summary_and_rejected_alternatives_from_a_table_or_bullets()
 {
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        let doc = std::fs::read_to_string(&path).unwrap();
        let name = format!("docs/adr/{}", path.file_name().unwrap().to_string_lossy());
        let num = &name["docs/adr/ADR-".len().."docs/adr/ADR-".len() + 4];
        let x = AdrAdapter.extract(&name, doc.as_bytes()).unwrap();
        let mut expected = vec![format!("adr-{num}-summary")];
        expected.extend(
            decision_lines(&doc)
                .iter()
                .map(|d| format!("adr-{num}-{d}")),
        );
        if doc.contains("\n## Rejected alternatives") {
            expected.push(format!("adr-{num}-rejected"));
        }
        assert_eq!(keys(&x), expected, "{name}");
    }
    // A table: one row per alternative, `alternative: why not`.
    let x = adr("ADR-0046");
    let rejected = get(&x, "adr-0046-rejected");
    assert_eq!(rejected.fragment.as_deref(), Some("rejected-alternatives"));
    let rows: Vec<&str> = rejected.text.lines().skip(1).collect();
    assert_eq!(rows.len(), 15, "{}", rejected.text);
    assert!(
        rows.iter().all(|r| r.starts_with("- ") && r.contains(": ")),
        "{rows:?}"
    );
    assert!(
        !rejected.text.contains("| Alternative"),
        "the header row is not an alternative"
    );
    // Bullets: one row per top-level bullet, its continuation joined.
    let x = adr("ADR-0023");
    let rejected = get(&x, "adr-0023-rejected");
    let rows: Vec<&str> = rejected.text.lines().skip(1).collect();
    assert_eq!(rows.len(), 7, "{}", rejected.text);
    assert!(
        rows[0].starts_with("- One merged binary for standalone. Keeps ADR-0019's"),
        "{}",
        rows[0]
    );
    assert!(
        rows[0].ends_with("from the four that run everywhere else."),
        "{}",
        rows[0]
    );
}

#[test]
fn no_extracted_text_is_cut_or_names_its_path() {
    let mut longest = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let doc = std::fs::read_to_string(&path).unwrap();
        let x = AdrAdapter
            .extract(&format!("docs/adr/{file}"), doc.as_bytes())
            .unwrap();
        for item in &x.items {
            assert!(
                !item.text.contains(&file),
                "{} names its file",
                item.claim_key
            );
            assert!(!item.text.ends_with('…'), "{} is cut", item.claim_key);
            if item.fragment.as_deref().is_some_and(|f| f.starts_with('d')) {
                assert!(
                    !item.text.contains("Status:"),
                    "{} carries the status",
                    item.claim_key
                );
                longest = longest.max(item.text.chars().count());
            }
        }
    }
    assert!(
        longest > 2_700,
        "a decision longer than the old 900-character cut is whole: {longest}"
    );
    // The last words of a long decision are in its text.
    let x = adr("ADR-0057");
    let d8 = get(&x, "adr-0057-d8");
    assert!(
        d8.text
            .starts_with("ADR-0057 d8: The run diffs the adapter's items"),
        "{}",
        d8.text
    );
    assert!(
        d8.text
            .ends_with("a second run at the same rev writes nothing."),
        "{}",
        d8.text
    );
    // The table row at column 0 inside d8 stays inside it.
    assert!(
        d8.text.contains("equivalent | the hash differs"),
        "{}",
        d8.text
    );
    let d14 = get(&x, "adr-0057-d14");
    assert!(
        d14.text.contains("7. Re-sync after an ADR changes."),
        "{}",
        d14.text
    );
    assert!(
        d14.text
            .ends_with("a renumbering costs a review per moved handle."),
        "{}",
        d14.text
    );
}

#[test]
fn the_summary_carries_the_title_the_status_the_context_and_each_headline() {
    let x = adr("ADR-0001");
    let s = get(&x, "adr-0001-summary");
    assert_eq!(s.fragment, None);
    let lines: Vec<&str> = s.text.lines().collect();
    assert_eq!(
        lines[0],
        "ADR-0001 — The constitution: four rules, one bounded development exception"
    );
    assert!(
        lines[1]
            .starts_with("Status: accepted design · 2026-10-29 · amended by ADR-0025, ADR-0037"),
        "{}",
        lines[1]
    );
    assert!(
        s.text
            .contains("Open Memory Lab's governing rules are the product")
    );
    assert!(
        s.text
            .contains("\nDecisions:\nd1 — R1, the verb ceiling.\n"),
        "{}",
        s.text
    );
    assert!(
        !s.text.contains("Release:"),
        "the release line is in the span, not the text"
    );
}

#[test]
fn amends_lines_state_edges_from_the_summary_to_each_named_decision() {
    let x = adr("ADR-0046");
    let to: Vec<String> = x
        .edges
        .iter()
        .map(|e: &EdgeStatement| {
            assert_eq!(
                (e.from.as_str(), e.label.as_str()),
                ("adr-0046-summary", "amends")
            );
            e.to.clone()
        })
        .collect();
    assert_eq!(
        to,
        [
            "adr-0008-d5",
            "adr-0009-d1",
            "adr-0009-d3",
            "adr-0009-d5",
            "adr-0021-d5",
            "adr-0029-d2",
            "adr-0039-d2",
            "adr-0039-d9"
        ]
    );
    // A record named without a decision points at the summary.
    let x = adr("ADR-0045");
    assert!(x.edges.iter().any(|e| e.to == "adr-0010-d1"));
    assert!(x.edges.iter().any(|e| e.to == "adr-0026-d15"));
    assert!(
        !x.edges.iter().any(|e| e.to == "adr-0026-d21"),
        "a handle after a colon is commentary"
    );
    assert!(
        x.edges.iter().all(|e| !e.to.starts_with("adr-0045-")),
        "a record never amends itself"
    );
}

#[test]
fn the_md_hash_ignores_reflow_and_changes_with_a_word() {
    let a = md_hash("The anchor is part of the signed\nsubmission, immutable per version.");
    assert_eq!(
        a,
        md_hash("The anchor is part of the  signed submission,\r\n  immutable per version.  ")
    );
    assert_ne!(
        a,
        md_hash("The anchor is part of the signed submission, mutable per version.")
    );
    assert_ne!(
        a,
        md_hash("The anchor is part of the signed submission; immutable per version.")
    );
    assert_eq!(
        md_hash("cafe\u{301} au lait"),
        md_hash("caf\u{e9} au lait"),
        "NFC first"
    );
    assert!(a.starts_with("sha256:") && a.len() == 71);
}

#[test]
fn the_headings_adapter_slugs_repeated_headings_as_github_does() {
    let doc = "---\ntitle: x\n---\n# Guide\n\nintro\n\n## Set up\n\nOne.\n\n### `oml sync` — the CLI!\n\nTwo [link](https://x.example).\n\n## Set up\n\nThree.\n\n## Empty\n\n## Set up\n\n<!-- hidden\n## Not a heading\n-->\nFour.\n\n```\n## inside a fence\n```\n";
    let x = HeadingsAdapter
        .extract("docs/guide-book.md", doc.as_bytes())
        .unwrap();
    let got: Vec<(&str, Option<&str>)> = x
        .items
        .iter()
        .map(|i| (i.claim_key.as_str(), i.fragment.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("doc/docs/guide-book/set-up", Some("set-up")),
            (
                "doc/docs/guide-book/oml-sync--the-cli",
                Some("oml-sync--the-cli")
            ),
            ("doc/docs/guide-book/set-up-1", Some("set-up-1")),
            ("doc/docs/guide-book/set-up-2", Some("set-up-2")),
        ]
    );
    assert_eq!(
        x.items[1].text,
        "Guide > Set up > oml sync — the CLI!\n\nTwo link."
    );
    assert_eq!(x.items[0].lines, (8, 10));
    assert!(
        x.items[3].text.ends_with("Four.\n## inside a fence"),
        "{}",
        x.items[3].text
    );
    assert!(x.edges.is_empty());
}

#[test]
fn the_lines_adapter_cuts_blocks_of_at_most_40_lines_at_blank_lines() {
    let mut doc = String::new();
    for p in 0..5 {
        for l in 0..12 {
            doc.push_str(&format!("p{p} l{l}\n"));
        }
        doc.push('\n');
    }
    let x = LinesAdapter
        .extract("notes/log.md", doc.as_bytes())
        .unwrap();
    let frags: Vec<&str> = x
        .items
        .iter()
        .map(|i| i.fragment.as_deref().unwrap())
        .collect();
    assert_eq!(frags, ["L1-L38", "L40-L64"]);
    assert_eq!(x.items[0].claim_key, "doc/notes/log/l1-l38");
}

#[test]
fn a_claim_key_over_128_characters_ends_in_a_digest() {
    let long = format!("doc/{}/x", "a".repeat(200));
    let key = fit_key(&long);
    assert_eq!(key.len(), 128);
    assert!(key.starts_with(&long[..111]));
    assert_eq!(&key[111..112], "-");
    assert!(key[112..].chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(fit_key(&format!("{long}y")), key);
    assert_eq!(fit_key("adr-0001-d1"), "adr-0001-d1");
}

/// The repository's own records: every decision under `## Decision` is an
/// item, whole, and each record with a rejected-alternatives section has
/// its item (the corpus ADR-0057 d14 loads).
#[test]
fn every_record_in_docs_adr_extracts_every_decision() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/adr");
    let mut records = 0;
    let mut decisions = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !AdrAdapter.accepts(&name) {
            continue;
        }
        records += 1;
        let doc = std::fs::read_to_string(&path).unwrap();
        let x = AdrAdapter
            .extract(&format!("docs/adr/{name}"), doc.as_bytes())
            .unwrap();
        let expected = decision_lines(&doc);
        decisions += expected.len();
        let got: Vec<&str> = x
            .items
            .iter()
            .filter_map(|i| i.fragment.as_deref())
            .filter(|f| *f != "rejected-alternatives")
            .collect();
        assert_eq!(got, expected, "{name}");
        assert_eq!(
            x.items
                .iter()
                .any(|i| i.fragment.as_deref() == Some("rejected-alternatives")),
            doc.contains("\n## Rejected alternatives"),
            "{name}"
        );
    }
    assert!(
        records >= 45 && decisions >= 400,
        "{records} records, {decisions} decisions"
    );
}

/// ADR-0057 d14.4 and d16.5: the load of OML's records at `ca61c9e`, a
/// fixture of that commit's `docs/adr`. The record says 117 edges; its
/// count missed the handles after the first in a parenthesis of `;` parts
/// (ADR-0049's `(d4: …; d6: …; d8: …; d10: …)`), which this parser reads.
#[test]
fn the_adr_load_at_ca61c9e_plans_523_items_and_133_amends_edges() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync/adr-ca61c9e");
    let mut items = Vec::new();
    let mut edges = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    for name in &names {
        let x = AdrAdapter
            .extract(
                &format!("docs/adr/{name}"),
                &std::fs::read(dir.join(name)).unwrap(),
            )
            .unwrap();
        items.extend(x.items);
        edges.extend(x.edges);
    }
    let count = |f: &dyn Fn(&Item) -> bool| items.iter().filter(|i| f(i)).count();
    assert_eq!(names.len(), 45);
    assert_eq!(items.len(), 523);
    assert_eq!(count(&|i| i.fragment.is_none()), 45);
    assert_eq!(
        count(&|i| i.fragment.as_deref() == Some("rejected-alternatives")),
        43
    );
    assert_eq!(edges.len(), 133);
    assert!(edges.iter().all(|e| e.label == "amends"));
    let keys: std::collections::HashSet<&str> =
        items.iter().map(|i| i.claim_key.as_str()).collect();
    assert!(
        edges.iter().all(|e| keys.contains(e.to.as_str())),
        "none unresolved"
    );
}

#[test]
fn a_path_mapped_lossily_into_a_key_carries_a_digest_of_the_path() {
    let doc = b"# T\n\n## S\n\nBody.\n";
    let a = HeadingsAdapter.extract("docs/A B.md", doc).unwrap();
    let b = HeadingsAdapter.extract("docs/a-b.md", doc).unwrap();
    assert_eq!(b.items[0].claim_key, "doc/docs/a-b/s");
    assert_ne!(a.items[0].claim_key, b.items[0].claim_key);
    assert!(
        a.items[0].claim_key.starts_with("doc/docs/a-b-"),
        "{}",
        a.items[0].claim_key
    );
}

#[test]
fn decision_prose_outside_a_dn_bullet_is_warned_not_loaded() {
    let doc = "# ADR-0009 — N\n\n**Status:** accepted\n\n## Context\n\nC.\n\n## Decision\n\n- **d1 — One.** Body.\n\n### Measured\n\nThe default stays.\n\n- **Measured on 2026-11-05.** It binds.\n";
    let x = AdrAdapter
        .extract("docs/adr/ADR-0009-n.md", doc.as_bytes())
        .unwrap();
    assert_eq!(x.items.len(), 2, "the summary and d1");
    assert_eq!(x.warnings.len(), 1, "{:?}", x.warnings);
    assert!(
        x.warnings[0].contains("3 line(s) under ## Decision outside a dN bullet"),
        "{:?}",
        x.warnings
    );
}
