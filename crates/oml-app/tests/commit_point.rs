//! Every use case commits through `App::commit_requests`, the one helper
//! that drops a tenant's widen signals when an erasure, a demotion or a
//! trust change commits (ADR-0072 d8, as built by #244; review of #260,
//! R6). A direct `journal.commit*` elsewhere in the app would commit such
//! an event without the drop.

use std::path::{Path, PathBuf};

/// Whether `b` can be part of an identifier.
fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `text` with every comment, string literal and character literal blanked
/// to spaces, newlines and byte offsets kept, so a scan reads code alone:
/// a `//` inside a string does not start a comment, and a commit named in a
/// string or a comment is not a commit (#294). A raw string and a nested
/// block comment are read; a lifetime is not a character literal.
fn code_only(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut [u8], from: usize, to: usize| {
        for byte in &mut out[from..to.min(b.len())] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        let prev = |back: usize| i.checked_sub(back).map(|k| b[k]);
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = text[i..].find('\n').map_or(b.len(), |n| i + n);
                blank(&mut out, i, end);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let (mut depth, mut j) = (1, i + 2);
                while j < b.len() && depth > 0 {
                    if b[j..].starts_with(b"/*") {
                        depth += 1;
                        j += 2;
                    } else if b[j..].starts_with(b"*/") {
                        depth -= 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            }
            // A raw string: `r`, `br`, then hashes and a quote.
            b'r' if prev(1).is_none_or(|p| !ident(p))
                || (prev(1) == Some(b'b') && prev(2).is_none_or(|p| !ident(p))) =>
            {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let mut close = vec![b'"'];
                    close.extend(std::iter::repeat_n(b'#', hashes));
                    let start = i + 2 + hashes;
                    let end = text[start..]
                        .find(std::str::from_utf8(&close).expect("ascii"))
                        .map_or(b.len(), |n| start + n + close.len());
                    blank(&mut out, i, end);
                    i = end;
                } else {
                    i += 1;
                }
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                blank(&mut out, i, j + 1);
                i = j + 1;
            }
            b'\'' => {
                // A character literal is a quote, one character or an escape,
                // and a quote; anything else is a lifetime.
                let width = text[i + 1..].chars().next().map_or(0, char::len_utf8);
                // An escape is a backslash and the escaped character, which
                // may itself be a quote, so the closing quote is searched
                // for from the byte after them (#312).
                let literal_end = if b.get(i + 1) == Some(&b'\\') {
                    text.get(i + 3..)
                        .and_then(|rest| rest.find('\'').map(|n| i + 3 + n + 1))
                } else if width > 0 && b.get(i + 1 + width) == Some(&b'\'') {
                    Some(i + 2 + width)
                } else {
                    None
                };
                if let Some(end) = literal_end {
                    blank(&mut out, i, end);
                    i = end;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("blanking keeps ASCII where it writes")
}

/// The byte offsets of every commit to something other than the App
/// itself: a `.commit(` or `.commit_chain(` whose receiver is not `self`,
/// and any path to `::commit` or `::commit_chain`, called, bound to a name
/// or passed as a function pointer, since the name is followed by anything
/// but an identifier character. `self.commit(..)` is the App's helper, or a
/// store's own method inside its impl. Comments, strings and character
/// literals are not read. A text scan, so a store reached under another
/// method name would pass it; none has one (L1, #268).
fn journal_commits(text: &str) -> Vec<usize> {
    let code = code_only(text);
    let mut found = Vec::new();
    for (at, _) in code.match_indices("commit") {
        let mut end = at + "commit".len();
        if code[end..].starts_with("_chain") {
            end += "_chain".len();
        }
        let after = code.as_bytes().get(end).copied();
        let before = &code[..at];
        if after.is_some_and(ident) || before.as_bytes().last().is_some_and(|&c| ident(c)) {
            continue;
        }
        // A path or a method call may be split from its name by whitespace
        // and line breaks (#312).
        let before = before.trim_end();
        let after = code[end..].trim_start().as_bytes().first().copied();
        if before.ends_with("::") {
            found.push(at);
        } else if before.ends_with('.') && after == Some(b'(') {
            // The receiver, which may end a previous line.
            let head = before[..before.len() - 1].trim_end();
            let receiver = head
                .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default();
            if receiver != "self" {
                found.push(at);
            }
        }
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
}

/// A path to the commit that is not called where it is named, a function
/// pointer, bypasses the helper as a call does (#294).
#[test]
fn the_guard_finds_a_commit_taken_as_a_function_pointer() {
    for bypass in [
        "let c = <J as JournalStore>::commit;\n        c(request).await?;",
        "let c = JournalStore::commit_chain;",
        "let c = <J as JournalStore>::commit ;",
        "run(&JournalStore::commit)",
        "[JournalStore::commit, JournalStore::commit_chain]",
    ] {
        assert!(!journal_commits(bypass).is_empty(), "missed: {bypass}");
    }
    for named_otherwise in [
        "App::commit_requests(&app, request)",
        "let c = Self::commit_all;",
        "use crate::ports::commit;",
    ] {
        // `use ...::commit;` names the function without taking it; the
        // guard flags it all the same, so a use case cannot import its way
        // round the helper.
        let flagged = !journal_commits(named_otherwise).is_empty();
        assert_eq!(
            flagged,
            named_otherwise.starts_with("use "),
            "{named_otherwise}"
        );
    }
}

/// What is not code is not scanned: a `//` inside a string earlier on a line
/// does not hide a later commit, and a commit named in a string, a block
/// comment or after a character literal's quote is read as what it is
/// (#294).
#[test]
fn the_guard_reads_code_and_skips_strings_and_comments() {
    for bypass in [
        "let url = \"http://example\"; self.journal.commit(request).await?;",
        "let q = '\"'; j.commit(request).await?;",
        "let raw = r#\"a // b\"#; j.commit(request).await?;",
        "let s = \"a \\\" // b\"; j.commit(request).await?;",
        "/* a block */ j.commit(request).await?;",
        "fn f<'a>(x: &'a str) { j.commit(request) }",
    ] {
        assert_eq!(journal_commits(bypass).len(), 1, "missed: {bypass}");
    }
    for not_code in [
        "let s = \"j.commit(request)\";",
        "/* j.commit(request) */",
        "/* a\n j.commit(request)\n */",
        "let s = r#\"j.commit(request)\"#;",
        "let s = \"a\n j.commit(request)\n b\";",
        "// j.commit(request) \"",
    ] {
        assert!(journal_commits(not_code).is_empty(), "flagged: {not_code}");
    }
    // A line comment ends at its line.
    assert_eq!(journal_commits("// a comment\nj.commit(request);").len(), 1);
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

/// An escaped quote is a character literal of four bytes, `'\''`, and its
/// closing quote is the one after the escaped character: the first quote
/// after the backslash is the escaped one. Read as two, the literal would
/// swallow the code after it, and a commit there would pass (#312).
#[test]
fn the_guard_reads_an_escaped_quote_character_literal_whole() {
    for bypass in [
        "let q = ['\\'','\"']; j.commit(x)",
        "let q = '\\''; j.commit(x)",
        "let q = '\\n'; j.commit(x)",
        "let q = '\\\\'; j.commit(x)",
        "let q = '\\u{1F600}'; j.commit(x)",
    ] {
        assert_eq!(journal_commits(bypass).len(), 1, "missed: {bypass}");
    }
    assert!(journal_commits("let q = '\\''; // j.commit(x)").is_empty());
}

/// Whitespace and line breaks may separate a path or a method call from the
/// name they lead to: `X::\n commit(..)`, `X:: commit(..)`, `j.\ncommit(..)`
/// and `j.commit (..)` commit as `X::commit(..)` does (#312).
#[test]
fn the_guard_finds_a_commit_split_by_whitespace_or_a_line_break() {
    for bypass in [
        "JournalStore::\n    commit(&self.journal, request).await?;",
        "JournalStore:: commit(&self.journal, request).await?;",
        "JournalStore::\n    commit_chain(&self.journal, requests).await?;",
        "j.\ncommit(request).await?;",
        "j.\n    commit_chain(requests).await?;",
        "j. commit(request).await?;",
        "j.commit (request).await?;",
        "j.commit\n    (request).await?;",
        "j.commit_chain (requests).await?;",
    ] {
        assert_eq!(journal_commits(bypass).len(), 1, "missed: {bypass}");
    }
    for helper_call in [
        "self.\ncommit(event).await?;",
        "self.commit (event).await?;",
        "self\n    .\n    commit_chain (links)\n    .await?;",
    ] {
        assert!(
            journal_commits(helper_call).is_empty(),
            "flagged: {helper_call}"
        );
    }
}
