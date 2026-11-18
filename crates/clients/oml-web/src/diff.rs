//! The console's own word comparison (ADR-0082 d6): the longest common
//! subsequence of two texts' whitespace-split words, no model, a removed
//! word struck (`<del>`) and an added one underlined (`<ins>`), so the
//! change does not rest on colour. The one-memory pages draw it as two
//! sides, Was and Now; To do's Judge draws it for a supersession as one
//! labelled block per part, name, description and body (ADR-0083 d2), and
//! the builder's edit form draws the same blocks after Preview (#410).

use std::fmt::Write as _;

use oml_page::web::text;

/// The most words the change compares; past it the two texts are shown
/// whole without marks, so a page never spends quadratic time on a long
/// text. A constant of the build, unmeasured.
const CHANGE_WORDS_MAX: usize = 2_000;

/// The words of `s`, split on whitespace.
fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// The longest common subsequence of two word lists, as a mark per word of
/// each: `true` where the word is kept. Past [`CHANGE_WORDS_MAX`] every
/// word is marked kept, so the texts are shown without marks.
fn kept(was: &[&str], now: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (was.len(), now.len());
    if n + m > CHANGE_WORDS_MAX {
        return (vec![true; n], vec![true; m]);
    }
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if was[i] == now[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut a, mut b) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if was[i] == now[j] {
            a[i] = true;
            b[j] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (a, b)
}

/// One side of the change: each word escaped, a dropped one struck through
/// (`del`) or an added one underlined (`ins`).
fn side(list: &[&str], marks: &[bool], tag: &str) -> String {
    list.iter()
        .zip(marks)
        .map(|(w, keep)| {
            if *keep {
                text(w)
            } else {
                format!("<{tag}>{}</{tag}>", text(w))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The change of `now` against `was` as one run of words, in order: a kept
/// word as it is, a removed one struck (`<del>`) and an added one
/// underlined (`<ins>`), each escaped; with the count of each. Past
/// [`CHANGE_WORDS_MAX`] the new text whole, unmarked.
pub(crate) fn word_diff(was: &str, now: &str) -> (String, usize, usize) {
    let (a, b) = (words(was), words(now));
    if a.len() + b.len() > CHANGE_WORDS_MAX {
        return (side(&b, &vec![true; b.len()], "ins"), 0, 0);
    }
    let (ka, kb) = kept(&a, &b);
    let (mut i, mut j) = (0, 0);
    let mut out: Vec<String> = Vec::with_capacity(a.len() + b.len());
    let (mut removed, mut added) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && !ka[i] {
            out.push(format!("<del>{}</del>", text(a[i])));
            removed += 1;
            i += 1;
        } else if j < b.len() && !kb[j] {
            out.push(format!("<ins>{}</ins>", text(b[j])));
            added += 1;
            j += 1;
        } else {
            // Both kept: the same word at the same place of the subsequence.
            out.push(text(b[j]));
            i += 1;
            j += 1;
        }
    }
    (out.join(" "), removed, added)
}

/// A version's parts as the change compares them: a skill's name and
/// description beside its body, and for every other kind the body alone.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Parts<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub body: &'a str,
}

/// The change of a held supersession against the version it replaces, as
/// Judge draws it (ADR-0082 d6; ADR-0083 d2): a `<details>` naming its
/// baseline, "Change against v2 · baseline: v2, the served tip", then one
/// labelled block per part the two versions carry, name, description and
/// body, each its own word run; a part that did not change says so.
pub(crate) fn labelled(was_version: u32, was: Parts<'_>, now: Parts<'_>) -> String {
    let mut blocks = String::new();
    let (mut removed, mut added) = (0, 0);
    for (label, was, now) in [
        ("Name", was.name, now.name),
        ("Description", was.description, now.description),
        ("Body", Some(was.body), Some(now.body)),
    ] {
        if was.is_none() && now.is_none() {
            continue;
        }
        let (was, now) = (was.unwrap_or_default(), now.unwrap_or_default());
        let (run, r, a) = word_diff(was, now);
        removed += r;
        added += a;
        let unchanged = if r + a == 0 {
            r#" <span class="n">unchanged</span>"#
        } else {
            ""
        };
        let _ = write!(
            blocks,
            r#"<div class="side"><p class="label">{label}{unchanged}</p><p class="t">{run}</p></div>"#
        );
    }
    format!(
        r#"<details class="oml-diff" open><summary>Change against v{was_version} <span class="n">baseline: v{was_version}, the served tip · {removed} words removed, {added} added</span></summary><div class="sides">{blocks}</div></details>"#
    )
}

/// The change against the version a successor replaces, at page density
/// (MemoryCard): a `<details>` that names its baseline and opens on two
/// named blocks, "Was" and "Now"; for a retirement, the reason beside what
/// stops being served.
pub(crate) fn change(
    was_version: i64,
    was: &str,
    now_version: u32,
    now: &str,
    retirement: bool,
) -> String {
    if retirement {
        return format!(
            r#"<details class="oml-diff" open><summary>What stops being served</summary><div class="sides"><div class="side"><p class="label">Reason · this version</p><p class="t">{}</p></div><div class="side"><p class="label">Stops being served · v{was_version}</p><p class="t">{}</p></div></div></details>"#,
            text(now),
            text(was)
        );
    }
    let (a, b) = (words(was), words(now));
    let (ka, kb) = kept(&a, &b);
    let removed = ka.iter().filter(|k| !**k).count();
    let added = kb.iter().filter(|k| !**k).count();
    format!(
        r#"<details class="oml-diff" open><summary>Change against v{was_version} <span class="n">baseline: v{was_version} · {removed} words removed, {added} added</span></summary><div class="sides"><div class="side"><p class="label">Was · v{was_version}</p><p class="t">{}</p></div><div class="side"><p class="label">Now · v{now_version}, this version</p><p class="t">{}</p></div></div></details>"#,
        side(&a, &ka, "del"),
        side(&b, &kb, "ins"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_diff_marks_removed_and_added_words() {
        let (run, removed, added) = word_diff("runs on ci-small", "runs on ci-medium now");
        assert_eq!(
            run,
            "runs on <del>ci-small</del> <ins>ci-medium</ins> <ins>now</ins>"
        );
        assert_eq!((removed, added), (1, 2));
        // Escaped, word by word: a crafted text puts no markup on the page.
        let (run, _, _) = word_diff("a <b>", "a <i>");
        assert_eq!(run, "a <del>&lt;b&gt;</del> <ins>&lt;i&gt;</ins>");
        let (same, removed, added) = word_diff("one two", "one  two");
        assert_eq!((same.as_str(), removed, added), ("one two", 0, 0));
    }

    #[test]
    fn the_judged_change_draws_each_part_in_its_own_labelled_block() {
        let block = labelled(
            2,
            Parts {
                name: Some("bump-deps"),
                description: Some("Bump the lockfile."),
                body: "Run cargo update.",
            },
            Parts {
                name: Some("bump-deps"),
                description: Some("Bump the lockfile weekly."),
                body: "Run cargo update.",
            },
        );
        assert!(
            block.contains("Change against v2 <span class=\"n\">baseline: v2, the served tip"),
            "{block}"
        );
        assert!(block.contains("<p class=\"label\">Name <span class=\"n\">unchanged</span>"));
        assert!(
            block.contains("<p class=\"label\">Description</p>"),
            "{block}"
        );
        assert!(block.contains("<del>lockfile.</del> <ins>lockfile</ins> <ins>weekly.</ins>"));
        assert!(block.contains("<p class=\"label\">Body <span class=\"n\">unchanged</span>"));
        // A memory has a body alone.
        let memory = labelled(
            1,
            Parts {
                body: "old",
                ..Parts::default()
            },
            Parts {
                body: "new",
                ..Parts::default()
            },
        );
        assert!(!memory.contains(">Name") && memory.contains(">Body</p>"));
    }

    #[test]
    fn the_change_strikes_a_removed_word_and_underlines_an_added_one() {
        let block = change(2, "runs on ci-small", 3, "runs on ci-medium now", false);
        assert!(block.contains("<del>ci-small</del>"), "{block}");
        assert!(
            block.contains("<ins>ci-medium</ins> <ins>now</ins>"),
            "{block}"
        );
        assert!(block.contains("1 words removed, 2 added"), "{block}");
        assert!(block.contains("Was · v2") && block.contains("Now · v3, this version"));
        let retired = change(1, "the old wiki", 2, "shut down", true);
        assert!(retired.contains("Stops being served · v1"), "{retired}");
    }
}
