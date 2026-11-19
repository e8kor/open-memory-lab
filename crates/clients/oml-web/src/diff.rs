//! The console's own word comparison (ADR-0082 d6): the longest common
//! subsequence of two texts' whitespace-split words, no model, a removed
//! word struck (`<del>`) and an added one underlined (`<ins>`), so the
//! change does not rest on colour. The one-memory pages draw it as two
//! sides, Was and Now; To do's Judge draws it for a supersession as one
//! labelled block per part, name, description and body (ADR-0083 d2), and
//! the builder's edit form draws the same blocks after Preview (#410).

use std::fmt::Write as _;

/// The most words and line breaks the change compares; past it the two
/// texts are shown whole without marks and the change says it is too long
/// to compare word by word, so a page never spends quadratic time on a
/// long text. A constant of the build, unmeasured.
const CHANGE_WORDS_MAX: usize = 2_000;

/// A line break, a token of its own so the change keeps the text's lines.
const BREAK: &str = "\n";

/// The words of `s`, split on whitespace.
fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// The tokens the change compares: the words of each line, with a
/// [`BREAK`] between two lines.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push(BREAK);
        }
        out.extend(line.split_whitespace());
    }
    out
}

/// What a comparison of two texts found (the review of PR #494, C-B2):
/// the same bytes, a change in whitespace alone, texts too long to compare
/// word by word, or a count of the words removed and added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Unchanged,
    WhitespaceOnly,
    TooLong { was: usize, now: usize },
    Changed { removed: usize, added: usize },
}

impl Outcome {
    /// The words a part's label carries after its name, `None` for a
    /// counted change, whose counts the summary carries.
    fn label(self) -> Option<String> {
        match self {
            Self::Unchanged => Some("unchanged".to_owned()),
            Self::WhitespaceOnly => Some("whitespace only".to_owned()),
            Self::TooLong { was, now } => Some(format!(
                "too long to compare word by word ({was} and {now} words)"
            )),
            Self::Changed { .. } => None,
        }
    }

    /// The words removed and added, `None` when none were counted.
    fn counts(self) -> Option<(usize, usize)> {
        match self {
            Self::Unchanged | Self::WhitespaceOnly => Some((0, 0)),
            Self::TooLong { .. } => None,
            Self::Changed { removed, added } => Some((removed, added)),
        }
    }
}

/// The longest common subsequence of two token lists, as a mark per token
/// of each: `true` where the token is kept.
fn kept(was: &[&str], now: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (was.len(), now.len());
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

/// Two texts compared: each side's tokens with their marks, and the
/// outcome. "Unchanged" is decided by the bytes, never by the counts.
struct Compared<'a> {
    was: Vec<&'a str>,
    kept_was: Vec<bool>,
    now: Vec<&'a str>,
    kept_now: Vec<bool>,
    outcome: Outcome,
}

fn compare<'a>(was: &'a str, now: &'a str) -> Compared<'a> {
    let (a, b) = (tokens(was), tokens(now));
    let whole = |a: &[&str], b: &[&str]| (vec![true; a.len()], vec![true; b.len()]);
    let within = a.len() + b.len() <= CHANGE_WORDS_MAX;
    let (outcome, (ka, kb)) = if was == now {
        (Outcome::Unchanged, whole(&a, &b))
    } else if words(was) == words(now) {
        let marks = if within { kept(&a, &b) } else { whole(&a, &b) };
        (Outcome::WhitespaceOnly, marks)
    } else if !within {
        let count = |t: &[&str]| t.iter().filter(|t| **t != BREAK).count();
        (
            Outcome::TooLong {
                was: count(&a),
                now: count(&b),
            },
            whole(&a, &b),
        )
    } else {
        let (ka, kb) = kept(&a, &b);
        let dropped = |t: &[&str], k: &[bool]| {
            t.iter()
                .zip(k)
                .filter(|(t, k)| !**k && **t != BREAK)
                .count()
        };
        (
            Outcome::Changed {
                removed: dropped(&a, &ka),
                added: dropped(&b, &kb),
            },
            (ka, kb),
        )
    };
    Compared {
        was: a,
        kept_was: ka,
        now: b,
        kept_now: kb,
        outcome,
    }
}

/// Draws a run of `(token, mark)`: a word escaped with hidden characters
/// shown, a kept break as `<br>`, and a marked token inside `<tag>` (a
/// marked break as "↵", followed by `<br>` when it is added).
fn render<'a>(items: impl IntoIterator<Item = (&'a str, Option<&'static str>)>) -> String {
    let mut out = String::new();
    let mut line_start = true;
    for (token, mark) in items {
        if token == BREAK {
            match mark {
                None => out.push_str("<br>"),
                Some("del") => {
                    if !line_start {
                        out.push(' ');
                    }
                    out.push_str("<del>↵</del>");
                    line_start = false;
                    continue;
                }
                Some(tag) => {
                    if !line_start {
                        out.push(' ');
                    }
                    let _ = write!(out, "<{tag}>↵</{tag}><br>");
                }
            }
            line_start = true;
            continue;
        }
        if !line_start {
            out.push(' ');
        }
        match mark {
            None => out.push_str(&shown(token)),
            Some(tag) => {
                let _ = write!(out, "<{tag}>{}</{tag}>", shown(token));
            }
        }
        line_start = false;
    }
    out
}

/// One side of the change: a dropped token struck through (`del`) or an
/// added one underlined (`ins`).
fn side(list: &[&str], marks: &[bool], tag: &'static str) -> String {
    render(
        list.iter()
            .zip(marks)
            .map(|(t, keep)| (*t, (!keep).then_some(tag))),
    )
}

/// The change of `now` against `was` as one run, in order: a kept token as
/// it is, a removed one struck (`<del>`) and an added one underlined
/// (`<ins>`), each escaped, the lines kept; with its outcome. Past
/// [`CHANGE_WORDS_MAX`] the new text whole, unmarked.
pub(crate) fn word_diff(was: &str, now: &str) -> (String, Outcome) {
    let c = compare(was, now);
    let (a, b, ka, kb) = (&c.was, &c.now, &c.kept_was, &c.kept_now);
    let mut items: Vec<(&str, Option<&'static str>)> = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && !ka[i] {
            items.push((a[i], Some("del")));
            i += 1;
        } else if j < b.len() && !kb[j] {
            items.push((b[j], Some("ins")));
            j += 1;
        } else if j < b.len() {
            // Both kept: the same token at the same place of the subsequence.
            items.push((b[j], None));
            i += 1;
            j += 1;
        } else {
            i += 1;
        }
    }
    (render(items), c.outcome)
}

/// A text's hidden characters drawn as marked code points (ADR-0082 d6;
/// the review of PR #494, 8): zero-width characters and tag characters,
/// which no reader sees, read as `<span class="oml-hidden">U+200B</span>`.
pub(crate) fn shown(s: &str) -> String {
    oml_page::web::text_shown(s)
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
    let mut counts = Some((0, 0));
    for (label, was, now) in [
        ("Name", was.name, now.name),
        ("Description", was.description, now.description),
        ("Body", Some(was.body), Some(now.body)),
    ] {
        if was.is_none() && now.is_none() {
            continue;
        }
        let (was, now) = (was.unwrap_or_default(), now.unwrap_or_default());
        let (run, outcome) = word_diff(was, now);
        counts = counts
            .zip(outcome.counts())
            .map(|((r, a), (r2, a2))| (r + r2, a + a2));
        let note = outcome
            .label()
            .map(|l| format!(r#" <span class="n">{l}</span>"#))
            .unwrap_or_default();
        let _ = write!(
            blocks,
            r#"<div class="side"><p class="label">{label}{note}</p><p class="t">{run}</p></div>"#
        );
    }
    // A part too long to compare draws no counts for the whole change.
    let counted = counts
        .map(|(removed, added)| format!(" · {}", counted(removed, added)))
        .unwrap_or_default();
    format!(
        r#"<details class="oml-diff" open><summary>Change against v{was_version} <span class="n">baseline: v{was_version}, the served tip{counted}</span></summary><div class="sides">{blocks}</div></details>"#
    )
}

/// A change's counts, "1 word removed, 2 words added": one word is one
/// word, any other count words.
fn counted(removed: usize, added: usize) -> String {
    let words = |n: usize| if n == 1 { "word" } else { "words" };
    format!(
        "{removed} {} removed, {added} {} added",
        words(removed),
        words(added)
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
            side(&tokens(now), &vec![true; tokens(now).len()], "ins"),
            side(&tokens(was), &vec![true; tokens(was).len()], "del")
        );
    }
    let c = compare(was, now);
    let found = match c.outcome {
        Outcome::Changed { removed, added } => counted(removed, added),
        other => other.label().unwrap_or_default(),
    };
    format!(
        r#"<details class="oml-diff" open><summary>Change against v{was_version} <span class="n">baseline: v{was_version} · {found}</span></summary><div class="sides"><div class="side"><p class="label">Was · v{was_version}</p><p class="t">{}</p></div><div class="side"><p class="label">Now · v{now_version}, this version</p><p class="t">{}</p></div></div></details>"#,
        side(&c.was, &c.kept_was, "del"),
        side(&c.now, &c.kept_now, "ins"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_diff_marks_removed_and_added_words() {
        let (run, outcome) = word_diff("runs on ci-small", "runs on ci-medium now");
        assert_eq!(
            run,
            "runs on <del>ci-small</del> <ins>ci-medium</ins> <ins>now</ins>"
        );
        assert_eq!(
            outcome,
            Outcome::Changed {
                removed: 1,
                added: 2
            }
        );
        // Escaped, word by word: a crafted text puts no markup on the page.
        let (run, _) = word_diff("a <b>", "a <i>");
        assert_eq!(run, "a <del>&lt;b&gt;</del> <ins>&lt;i&gt;</ins>");
        let (same, outcome) = word_diff("one two", "one two");
        assert_eq!((same.as_str(), outcome), ("one two", Outcome::Unchanged));
    }

    /// "Unchanged" is the same bytes: a change in whitespace alone says so,
    /// a text past the cap says it is too long to compare and counts
    /// nothing, and the lines of a text are kept (the review of PR #494,
    /// C-B2).
    #[test]
    fn the_word_diff_says_unchanged_only_for_the_same_text() {
        let (run, outcome) = word_diff("one two", "one  two");
        assert_eq!(
            (run.as_str(), outcome),
            ("one two", Outcome::WhitespaceOnly)
        );
        let (run, outcome) = word_diff("one\ntwo", "one two");
        assert_eq!(outcome, Outcome::WhitespaceOnly);
        assert_eq!(run, "one <del>↵</del> two");
        let (run, outcome) = word_diff("a\nb", "a\nc");
        assert_eq!(run, "a<br><del>b</del> <ins>c</ins>");
        assert_eq!(
            outcome,
            Outcome::Changed {
                removed: 1,
                added: 1
            }
        );
        let long = "word ".repeat(1_500);
        let longer = format!("{long}more");
        let (_, outcome) = word_diff(&long, &longer);
        assert_eq!(
            outcome,
            Outcome::TooLong {
                was: 1_500,
                now: 1_501
            }
        );
        let block = labelled(
            1,
            Parts {
                body: &long,
                ..Parts::default()
            },
            Parts {
                body: &longer,
                ..Parts::default()
            },
        );
        assert!(
            block.contains("too long to compare word by word (1500 and 1501 words)"),
            "{block}"
        );
        assert!(!block.contains("words removed"), "{block}");
        assert!(!block.contains("unchanged"), "{block}");
        let two = change(1, &long, 2, &longer, false);
        assert!(two.contains("too long to compare word by word"), "{two}");
        let spaced = change(1, "a b", 2, "a  b", false);
        assert!(spaced.contains("whitespace only"), "{spaced}");
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
        // One word is one word; any other count is words.
        assert!(
            memory.contains("the served tip · 1 word removed, 1 word added"),
            "{memory}"
        );
        assert!(
            block.contains("the served tip · 1 word removed, 2 words added"),
            "{block}"
        );
    }

    #[test]
    fn the_change_strikes_a_removed_word_and_underlines_an_added_one() {
        let block = change(2, "runs on ci-small", 3, "runs on ci-medium now", false);
        assert!(block.contains("<del>ci-small</del>"), "{block}");
        assert!(
            block.contains("<ins>ci-medium</ins> <ins>now</ins>"),
            "{block}"
        );
        assert!(block.contains("1 word removed, 2 words added"), "{block}");
        let one = change(1, "a b", 2, "a c", false);
        assert!(one.contains("1 word removed, 1 word added"), "{one}");
        assert!(block.contains("Was · v2") && block.contains("Now · v3, this version"));
        let retired = change(1, "the old wiki", 2, "shut down", true);
        assert!(retired.contains("Stops being served · v1"), "{retired}");
    }
}
