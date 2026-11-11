//! Every use case commits through `App::commit_requests`, the one helper
//! that drops a tenant's widen signals when an erasure, a demotion or a
//! trust change commits (ADR-0072 d8, as built by #244; review of #260,
//! R6). A direct `journal.commit*` elsewhere in the app would commit such
//! an event without the drop.

use std::path::Path;

/// The byte offsets of every `self.journal` followed, across whitespace, by
/// `.commit`.
fn journal_commits(text: &str) -> Vec<usize> {
    text.match_indices("self.journal")
        .filter(|(at, m)| text[at + m.len()..].trim_start().starts_with(".commit"))
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn every_use_case_commits_through_the_helper_that_drops_widen_signals() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let app = std::fs::read_to_string(src.join("app.rs")).expect("app.rs");
    let start = app.find("async fn commit_requests(").expect("the helper");
    let end = start + app[start..].find("\n    }\n").expect("its end");
    let mut outside = Vec::new();
    for at in journal_commits(&app) {
        if !(start..end).contains(&at) {
            outside.push(format!("app.rs at byte {at}"));
        }
    }
    assert_eq!(
        journal_commits(&app[start..end]).len(),
        2,
        "the helper commits one request or a chain"
    );
    for entry in std::fs::read_dir(src.join("app")).expect("src/app") {
        let path = entry.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a source file");
        for at in journal_commits(&text) {
            outside.push(format!("{} at byte {at}", path.display()));
        }
    }
    assert!(
        outside.is_empty(),
        "a use case commits to the journal around the helper: {outside:?}"
    );
}
