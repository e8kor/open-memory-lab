//! Every use case commits through `App::commit_requests`, the one helper
//! that drops a tenant's widen signals when an erasure, a demotion or a
//! trust change commits (ADR-0072 d8, as built by #244; review of #260,
//! R6). A direct `journal.commit*` elsewhere in the app would commit such
//! an event without the drop.

use std::path::{Path, PathBuf};

/// The byte offsets of every commit to something other than the App
/// itself: a `.commit(` or `.commit_chain(` whose receiver is not `self`,
/// and any path call `::commit(` or `::commit_chain(`. `self.commit(..)`
/// is the App's helper, or a store's own method inside its impl. Line
/// comments are skipped. A text scan, so a store reached under another
/// method name would pass it; none has one (L1, #268).
fn journal_commits(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let code = line.find("//").map_or(line, |c| &line[..c]);
        for name in ["commit(", "commit_chain("] {
            for (at, _) in code.match_indices(name) {
                let before = &code[..at];
                if before.ends_with("::") {
                    found.push(offset + at);
                } else if before.ends_with('.') {
                    // The receiver, which may end a previous line.
                    let head = text[..offset + at - 1].trim_end();
                    let receiver = head
                        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                        .unwrap_or_default();
                    if receiver != "self" {
                        found.push(offset + at);
                    }
                }
            }
        }
        offset += line.len();
    }
    found
}

/// Every `.rs` file under `dir`, at any depth.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("a source directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            out.extend(sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

#[test]
fn every_use_case_commits_through_the_helper_that_drops_widen_signals() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let app = std::fs::read_to_string(src.join("app.rs")).expect("app.rs");
    let start = app.find("async fn commit_requests(").expect("the helper");
    let end = start + app[start..].find("\n    }\n").expect("its end");
    assert_eq!(
        journal_commits(&app[start..end]).len(),
        2,
        "the helper commits one request or a chain"
    );
    let mut outside = Vec::new();
    for path in sources(&src) {
        let text = std::fs::read_to_string(&path).expect("a source file");
        for at in journal_commits(&text) {
            let in_helper = path.ends_with("src/app.rs") && (start..end).contains(&at);
            if !in_helper {
                outside.push(format!("{} at byte {at}", path.display()));
            }
        }
    }
    assert!(
        outside.is_empty(),
        "a use case commits to the journal around the helper: {outside:?}"
    );
}

/// Each way round the helper that the review of #260 named (L1, #268): a
/// local binding of the journal, a fully qualified call, a chain commit and
/// a commit on a line of its own.
#[test]
fn the_guard_finds_a_commit_through_a_binding_a_qualified_call_or_a_chain() {
    for bypass in [
        "let j = &self.journal;\n        j.commit(request).await?;",
        "JournalStore::commit(&self.journal, request).await?;",
        "<J as JournalStore>::commit_chain(&self.journal, requests).await?;",
        "self.journal.commit_chain(requests).await?;",
        "self\n            .journal\n            .commit(request)\n            .await?;",
    ] {
        assert_eq!(journal_commits(bypass).len(), 1, "missed: {bypass}");
    }
    for helper_call in [
        "self.commit(event, fold, None).await?;",
        "self\n            .commit_chain(links)\n            .await?;",
        "// a comment naming j.commit(request)",
    ] {
        assert!(
            journal_commits(helper_call).is_empty(),
            "flagged: {helper_call}"
        );
    }
}
