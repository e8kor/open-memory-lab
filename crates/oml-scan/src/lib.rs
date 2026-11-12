//! The deterministic content scan, rule set `oml-scan/1.2` (ADR-0065; `/1` before the 2026-11-10 amendment, `/1.1` before #226).
//!
//! Rules, not a model: the same text gives the same findings under the same
//! rule-set version (admission.md). A finding names its class, its rule and
//! where it fired (a field and a line) and never the text it matched, so a
//! response, a log line, the journal or a metric cannot carry the secret it
//! found (d8).
//!
//! A leaf crate: it links no workspace crate, so the server's scanner port
//! adapter, `oml-domain`'s anchor path rule and `oml-cli`'s sync read one
//! rule list (d4, amending ADR-0006 d6 by a second leaf beside
//! `oml-guardrail`). Which fields are scanned is the caller's; this crate
//! scans what it is handed.

#![forbid(unsafe_code)]

mod path;
mod rules;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{LazyLock, Mutex};

use regex::Regex;

pub use path::secret_path;
pub use rules::Action;
use rules::{Check, LETTER_PHRASES, PLACEHOLDERS, RULES, Rule, Tail};

/// The rule-set version every verdict is journalled under (ADR-0065 d1).
pub const RULESET: &str = "oml-scan/1.2";

/// The largest text one scan reads: 1 MiB. A longer field is vetoed as
/// `format.scan_bound` rather than scanned in part (d9).
pub const MAX_SCAN_BYTES: usize = 1 << 20;

/// More invisible format characters than this in one field is a hold (d5).
pub const INVISIBLE_LIMIT: usize = 3;

/// Findings named in one reason string; the rest are counted (d8).
const REASON_FINDINGS: usize = 5;

/// One finding: the class a review policy can name, the rule that fired,
/// what it does, and where. It holds no matched text by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub class: &'static str,
    pub rule: &'static str,
    pub action: Action,
    /// The field the caller named: `/body`, `/anchor/fragment`, a JSON
    /// pointer into a decoded value.
    pub field: String,
    /// The 1-based line of the field the match starts on.
    pub line: usize,
    /// The view the rule read when it fired (ADR-0074 d6, d23): `raw`, the
    /// field as written; `clean`, the removable characters deleted; `spaced`,
    /// each made a space; `letters`, the letters-only view.
    pub via: &'static str,
    /// The match's byte range in that view: a place, never the text.
    pub span: (usize, usize),
}

impl Finding {
    /// Where the finding fired: `/body line 3`.
    #[must_use]
    pub fn location(&self) -> String {
        format!("{} line {}", self.field, self.line)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} at {} line {}",
            self.class, self.rule, self.field, self.line
        )
    }
}

/// The verdict over every field of one submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Admit held; every hold finding, in field then position order.
    Hold(Vec<Finding>),
    /// Refuse; the veto findings only — a veto outranks any hold.
    Veto(Vec<Finding>),
}

impl Verdict {
    /// The reason string the port carries: up to five findings, `; `
    /// separated, then how many more (d8).
    #[must_use]
    pub fn reason(&self) -> Option<String> {
        let findings = match self {
            Self::Pass => return None,
            Self::Hold(f) | Self::Veto(f) => f,
        };
        let mut out = findings
            .iter()
            .take(REASON_FINDINGS)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        if findings.len() > REASON_FINDINGS {
            out.push_str(&format!("; and {} more", findings.len() - REASON_FINDINGS));
        }
        Some(out)
    }
}

/// What a deployment turns on beyond the default set (ADR-0065 d6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The `pii.*` rules (emails, phone numbers) as holds. Off by default.
    pub pii: bool,
}

/// Each rule's own regex. One set over every pattern built a combined
/// DFA that thrashed its cache on dense input and fell back to the slow
/// engine: 258 ms on a mebibyte of base64, where the rules one at a time,
/// each with its own literal prefilter, take about 9 ms (d9).
static COMPILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    RULES
        .iter()
        .map(|r| Regex::new(r.pattern).expect("the oml-scan/1.2 patterns compile"))
        .collect()
});

/// The letters-only view's phrase patterns, each with the rule it reports.
static LETTERS: LazyLock<Vec<(&'static Rule, Regex, Tail)>> = LazyLock::new(|| {
    LETTER_PHRASES
        .iter()
        .map(|(id, pattern, tail)| {
            let rule = RULES
                .iter()
                .find(|r| r.id == *id)
                .expect("a letters phrase names a rule");
            (
                rule,
                Regex::new(pattern).expect("the letters phrases compile"),
                *tail,
            )
        })
        .collect()
});

/// One run of letters and digits: what the letters-only view keeps.
static LETTER_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]+").expect("compiles"));

/// Every rule id the set can report, with its class and action: the closed
/// label set of the findings metric (d11).
#[must_use]
pub fn rule_ids() -> Vec<(&'static str, &'static str, Action)> {
    let mut out: Vec<_> = RULES.iter().map(|r| (r.class, r.id, r.action)).collect();
    out.push(("secret.assignment", "aws-secret-near-id", Action::Veto));
    out.push(("encoding.base64_blob", "base64-blob", Action::Hold));
    out.push(("unicode.invisible", "invisible-format", Action::Hold));
    out.push(("unicode.invisible", "tag-characters", Action::Hold));
    out.push(("unicode.invisible", "variation-run", Action::Hold));
    out.push(("format.scan_bound", "scan-bound", Action::Veto));
    out
}

/// The scanner: the compiled `oml-scan/1.2` set, the deployment's options,
/// and the findings counted by rule since start (d11).
#[derive(Debug, Default)]
pub struct RuleSet {
    options: Options,
    counts: Mutex<BTreeMap<(&'static str, &'static str, Action), u64>>,
}

impl RuleSet {
    /// The set with `options`. Compiles the patterns on first use in the
    /// process; a pattern that does not compile is a build defect, caught
    /// by this crate's tests, never a runtime state.
    #[must_use]
    pub fn new(options: Options) -> Self {
        LazyLock::force(&COMPILED);
        Self {
            options,
            counts: Mutex::default(),
        }
    }

    /// How many rules this set runs: every compiled rule, less the
    /// personal-data rules while they are off (d6, d11).
    #[must_use]
    pub fn rules(&self) -> usize {
        rule_ids().len()
            - if self.options.pii {
                0
            } else {
                RULES.iter().filter(|r| r.pii).count()
            }
    }

    /// The options this set was built with.
    #[must_use]
    pub const fn options(&self) -> Options {
        self.options
    }

    /// The version this set reports.
    #[must_use]
    pub const fn version(&self) -> &'static str {
        RULESET
    }

    /// Scan every field and decide (d7): any veto finding refuses; else any
    /// hold finding holds; else pass.
    #[must_use]
    pub fn scan(&self, fields: &[(&str, &str)]) -> Verdict {
        let mut vetoes = Vec::new();
        let mut holds = Vec::new();
        for &(field, text) in fields {
            for f in self.findings(field, text) {
                match f.action {
                    Action::Veto => vetoes.push(f),
                    Action::Hold => holds.push(f),
                }
            }
        }
        self.count(vetoes.iter().chain(&holds));
        if !vetoes.is_empty() {
            Verdict::Veto(vetoes)
        } else if !holds.is_empty() {
            Verdict::Hold(holds)
        } else {
            Verdict::Pass
        }
    }

    /// Every finding in one field, in position order. Counts nothing.
    #[must_use]
    pub fn findings(&self, field: &str, text: &str) -> Vec<Finding> {
        if text.len() > MAX_SCAN_BYTES {
            return vec![Finding {
                class: "format.scan_bound",
                rule: "scan-bound",
                action: Action::Veto,
                field: field.to_owned(),
                line: 1,
                via: "raw",
                span: (0, text.len()),
            }];
        }
        // The order is fixed (d14, amended 2026-11-10): first every pattern
        // and run rule over the text as written; then the invisible-character
        // rules over the text as written, since the views remove what they
        // count; then every pattern and run rule again over each normalised
        // view, veto and hold alike, keeping each rule no earlier pass
        // reported. The first view deletes the removable characters, which
        // undoes one inside a word; the second puts a space where each was,
        // which undoes one used as the space between words. A view that
        // normalises past the bound is vetoed as the text would be (d9).
        let views = normalised(text);
        if views.iter().any(|(_, v)| v.len() > MAX_SCAN_BYTES) {
            return vec![Finding {
                class: "format.scan_bound",
                rule: "scan-bound",
                action: Action::Veto,
                field: field.to_owned(),
                line: 1,
                via: "raw",
                span: (0, text.len()),
            }];
        }
        let mut out = self.matched(field, text, "raw");
        if let Some((span, rule)) = invisible(text) {
            out.push((
                span.0,
                code_finding(
                    "unicode.invisible",
                    rule,
                    Action::Hold,
                    field,
                    text,
                    span,
                    "raw",
                ),
            ));
        }
        for (via, view) in &views {
            for (start, f) in self.matched(field, view, via) {
                if !out.iter().any(|(_, g)| g.rule == f.rule) {
                    out.push((start, f));
                }
            }
        }
        // Last, the letters-only view (ADR-0074 d6): compatibility forms
        // decomposed, lower case, and every code point that is not a letter
        // or a digit dropped, combining marks, spaces, punctuation and
        // invisible characters alike. It is read by the injection phrases
        // alone, written without separators, so no character inside or
        // between the words hides one.
        for (start, f) in letters_findings(field, text) {
            if !out.iter().any(|(_, g)| g.rule == f.rule) {
                out.push((start, f));
            }
        }
        out.sort_by_key(|(start, f)| (*start, f.rule));
        out.into_iter().map(|(_, f)| f).collect()
    }

    /// The pattern and run rules over one text, with each finding's start.
    fn matched(&self, field: &str, text: &str, via: &'static str) -> Vec<(usize, Finding)> {
        let mut out: Vec<(usize, Finding)> = Vec::new();
        for (rule, re) in RULES.iter().zip(COMPILED.iter()) {
            // The personal-data rules do not run while they are off (d6).
            if rule.pii && !self.options.pii {
                continue;
            }
            if let Some(span) = first_match(rule, re, text) {
                out.push((
                    span.0,
                    code_finding(rule.class, rule.id, rule.action, field, text, span, via),
                ));
            }
        }
        if let Some(span) = base64_run(text) {
            out.push((
                span.0,
                code_finding(
                    "encoding.base64_blob",
                    "base64-blob",
                    Action::Hold,
                    field,
                    text,
                    span,
                    via,
                ),
            ));
        }
        if out.iter().any(|(_, f)| f.rule == "aws-access-key-id")
            && let Some(span) = aws_secret_near_id(text)
        {
            out.push((
                span.0,
                code_finding(
                    "secret.assignment",
                    "aws-secret-near-id",
                    Action::Veto,
                    field,
                    text,
                    span,
                    via,
                ),
            ));
        }
        out
    }

    /// Findings counted since start, by class, rule and action (d11).
    #[must_use]
    pub fn counts(&self) -> Vec<(&'static str, &'static str, Action, u64)> {
        self.counts
            .lock()
            .map(|c| c.iter().map(|(&(c, r, a), &n)| (c, r, a, n)).collect())
            .unwrap_or_default()
    }

    fn count<'a>(&self, findings: impl Iterator<Item = &'a Finding>) {
        if let Ok(mut counts) = self.counts.lock() {
            for f in findings {
                *counts.entry((f.class, f.rule, f.action)).or_default() += 1;
            }
        }
    }
}

/// Where the rule's first real finding lies, after its second look.
fn first_match(rule: &Rule, re: &Regex, text: &str) -> Option<(usize, usize)> {
    match rule.check {
        Check::None => re.find(text).map(|m| (m.start(), m.end())),
        Check::Entropy => re
            .captures_iter(text)
            .find(|c| c.get(1).is_some_and(|v| generated(v.as_str())))
            .and_then(|c| c.get(0))
            .map(|m| (m.start(), m.end())),
        Check::Password => re
            .captures_iter(text)
            .find(|c| match (c.get(1), c.get(2)) {
                (Some(user), Some(pass)) => real_password(user.as_str(), pass.as_str()),
                _ => false,
            })
            .and_then(|c| c.get(0))
            .map(|m| (m.start(), m.end())),
    }
}

fn code_finding(
    class: &'static str,
    rule: &'static str,
    action: Action,
    field: &str,
    text: &str,
    span: (usize, usize),
    via: &'static str,
) -> Finding {
    Finding {
        class,
        rule,
        action,
        field: field.to_owned(),
        line: line_of(text, span.0),
        via,
        span,
    }
}

/// The shortest base64 run the blob rule reads (d5).
const BLOB_MIN: usize = 400;

/// The first run of 400 or more base64 characters that passes the blob
/// test, counted by hand: one pass, no regex state (d9).
fn base64_run(text: &str) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let is = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'_' | b'-');
    let mut i = 0;
    while i < b.len() {
        if !is(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && is(b[i]) {
            i += 1;
        }
        if i - start >= BLOB_MIN && blob(&text[start..i]) {
            return Some((start, i));
        }
    }
    None
}

/// A 40-character base64 value within 200 bytes after an AWS access key
/// id, with the entropy test: the secret half of a pair pasted with no key
/// name (d3).
fn aws_secret_near_id(text: &str) -> Option<(usize, usize)> {
    static ID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:A3T[A-Z0-9]|AKIA|ASIA|ABIA|ACCA)[A-Z2-7]{16}").expect("compiles")
    });
    let b = text.as_bytes();
    let is = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/');
    for m in ID.find_iter(text) {
        let end = (m.end() + 200).min(b.len());
        let mut i = m.end();
        while i < end {
            if !is(b[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && is(b[i]) {
                i += 1;
            }
            if i - start == 40 && generated(&text[start..i]) {
                return Some((start, i));
            }
        }
    }
    None
}

/// Characters that change nothing a reader sees, so copy-paste or an
/// evasion can put them inside a token or a phrase: Unicode's default
/// ignorable code points — the soft hyphen, the combining grapheme joiner,
/// the Arabic letter mark, the Hangul fillers, the Mongolian vowel
/// separator, the zero-width spaces, joiners and directional marks, the
/// bidirectional controls, the word joiner and the invisible operators,
/// the variation selectors, the byte-order mark, the interlinear
/// annotation marks and the tag characters (d14, amended 2026-11-10).
fn removable(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFFB}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// The text with what hides a token or a phrase undone, none when it is
/// already that text: NBSP a space, NFKC, and the removable characters
/// deleted in one view, `clean`, and made spaces in the other, `spaced`
/// (d14). Not case-folded: the hold patterns match either case already,
/// and folding would hide an upper-case key from the veto patterns.
fn normalised(text: &str) -> Vec<(&'static str, String)> {
    use unicode_normalization::UnicodeNormalization as _;
    use unicode_normalization::{IsNormalized, is_nfkc_quick};
    let plain = text.is_ascii()
        || (!text.chars().any(|c| removable(c) || c == '\u{00A0}')
            && is_nfkc_quick(text.chars()) == IsNormalized::Yes);
    if plain {
        return Vec::new();
    }
    let view = |sep: Option<char>| -> String {
        text.chars()
            .filter_map(|c| {
                if removable(c) {
                    sep
                } else if c == '\u{00A0}' {
                    Some(' ')
                } else {
                    Some(c)
                }
            })
            .nfkc()
            .collect()
    };
    let mut views = vec![("clean", view(None)), ("spaced", view(Some(' ')))];
    views.dedup_by(|a, b| a.1 == b.1);
    views.retain(|(_, v)| v != text);
    views
}

/// A mark, as `NFKD` leaves it after its letter.
fn mark(c: char) -> bool {
    unicode_normalization::char::is_combining_mark(c)
}

/// Whether the character before byte `at` of `folded`, the marks that belong
/// to it aside, came from an ASCII letter: the word the phrase would
/// continue. `glue` says it for each byte of `folded`. The raw rules read
/// word boundaries with ASCII `\b`, and so does this: a digit, `_`, a
/// letter of another script and a punctuation mark are edges (#226).
fn glued_before(folded: &str, glue: &[bool], at: usize) -> bool {
    folded[..at]
        .char_indices()
        .rev()
        .find(|&(_, c)| !mark(c))
        .is_some_and(|(i, _)| glue[i])
}

/// Whether the character at or after byte `at` of `folded`, past marks,
/// came from an ASCII letter.
fn glued_after(folded: &str, glue: &[bool], at: usize) -> bool {
    folded[at..]
        .char_indices()
        .find(|&(_, c)| !mark(c))
        .is_some_and(|(i, _)| glue[at + i])
}

/// A line break: a line feed, a carriage return, a vertical tab, a form
/// feed, NEL, or a line or paragraph separator.
fn line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Whether a gap between two words of a phrase ends a clause: a line break,
/// or a comma, semicolon, colon, question or exclamation mark followed by a
/// space. `new, system, instructions:` is a list of three words.
fn clause_break(gap: &str) -> bool {
    let mut chars = gap.chars().peekable();
    while let Some(c) = chars.next() {
        if line_break(c) || (",;:!?".contains(c) && chars.peek().is_some_and(|n| n.is_whitespace()))
        {
            return true;
        }
    }
    false
}

/// Whether a gap holds a full stop and a space, the end of a sentence, or of
/// one letter of a word spelled `n. e. w.`. A separator inside a word
/// (`n.e.w`) is not one.
fn dot_break(gap: &str) -> bool {
    let mut chars = gap.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '.' && chars.peek().is_some_and(|n| n.is_whitespace()) {
            return true;
        }
    }
    false
}

/// The gap between two words of a phrase with a hyphenated line wrap read
/// as the hyphen alone: a line break that follows a hyphen splits a word,
/// and does not end a clause.
fn unwrap_hyphens(gap: &str) -> String {
    let mut out = String::with_capacity(gap.len());
    // Whether the last character kept that is not a blank is a hyphen: kept as
    // a flag, since a gap of many blanks must not be read again for each.
    let mut hyphen = false;
    for c in gap.replace("\r\n", "\n").chars() {
        if line_break(c) && hyphen {
            continue;
        }
        if !c.is_whitespace() {
            hyphen = c == '-';
        }
        out.push(c);
    }
    out
}

/// A character that is part of the line a heading, an emphasis, a quote, a
/// list item, a banner or a bracket makes, and no part of its words: any
/// character that is not a letter, a digit or a bar (which bounds a table
/// cell), a combining mark counting with its letter. A line break is not one.
fn line_mark(c: char) -> bool {
    // The enclosed alphanumerics are letters of Unicode's alphabetic property
    // and look like a symbol: a circled or squared letter is a bullet.
    let enclosed =
        matches!(c, '\u{2460}'..='\u{24FF}' | '\u{2776}'..='\u{2793}' | '\u{1F100}'..='\u{1F1FF}');
    !line_break(c) && (mark(c) || enclosed || (!c.is_alphanumeric() && c != '|'))
}

/// How many tags, or phrases, a line may carry on either side of the phrase
/// and still be its line. A tag is read whole, letters and all, so a line of
/// tags would be read again for each phrase on it (d9).
const SKIPS: usize = 8;

/// The length of the HTML tag that starts `s`, its `<` included: a name and
/// whatever attributes, up to the `>`, on the one line.
fn tag_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix('<')?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let end = rest.find(|c: char| c == '<' || c == '>' || line_break(c))?;
    rest[end..]
        .starts_with('>')
        .then(|| s.len() - rest.len() + end + 1)
}

/// The `new system instructions` phrase at the start of `s`, as the folded
/// text shows it, with a few marks between its words.
static PHRASE_HERE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:new|updated|real)[^\p{L}\p{N}]{0,8}system[^\p{L}\p{N}]{0,8}instructions")
        .expect("compiles")
});

/// Whether only what opens a line stands before the phrase on it: marks, a
/// list marker (a lone number or letter) and up
/// to `SKIPS` tags, back to the line break before it. `before` is the text up
/// to the phrase and is read backwards, so the cost is what the phrase's line
/// holds up to its first letter, and never the text before the line (d9: a
/// megabyte on one line is read once, not once per phrase).
fn opens_line(before: &str) -> bool {
    let mut rest = before;
    let (mut tags, mut marker) = (0, false);
    while let Some(c) = rest.chars().next_back() {
        if line_break(c) {
            return true;
        }
        let cut = rest.len() - c.len_utf8();
        if c == '>' && tags < SKIPS {
            let open = rest[..cut]
                .rfind(|d: char| d == '<' || d == '>' || line_break(d))
                .filter(|&i| rest[i..].starts_with('<'));
            if let Some(i) = open.filter(|&i| tag_len(&rest[i..]) == Some(rest.len() - i)) {
                rest = &rest[..i];
                tags += 1;
                continue;
            }
        }
        if !marker && c.is_alphanumeric() && !mark(c) {
            // A lone number or letter is a list marker, however a keycap, a
            // circle or a bracket wrote it: the folded text has it as one
            // digit run or one ASCII letter. Read at most ten characters back.
            let run: String = rest
                .chars()
                .rev()
                .take(10)
                .take_while(|&d| d.is_alphanumeric())
                .collect();
            if run.len() == 1 && run.bytes().all(|b| b.is_ascii_alphabetic())
                || (run.len() <= 9 && run.bytes().all(|b| b.is_ascii_digit()))
            {
                rest = &rest[..rest.len() - run.len()];
                marker = true;
                continue;
            }
        }
        if line_mark(c) {
            rest = &rest[..cut];
            continue;
        }
        return false;
    }
    true
}

/// Whether only what closes a line stands after the phrase on it: marks, and
/// up to `SKIPS` tags or further phrases, to the line break after it, read
/// forwards from `after` as far as the first letter.
fn closes_line(after: &str) -> bool {
    let mut rest = after;
    let mut skips = 0;
    while let Some(c) = rest.chars().next() {
        if line_break(c) {
            return true;
        }
        if skips < SKIPS {
            let skipped = if c == '<' {
                tag_len(rest)
            } else if c.is_alphanumeric() {
                PHRASE_HERE.find(rest).map(|m| m.end())
            } else {
                None
            };
            if let Some(n) = skipped {
                rest = &rest[n..];
                skips += 1;
                continue;
            }
        }
        if !line_mark(c) {
            return false;
        }
        rest = &rest[c.len_utf8()..];
    }
    true
}

/// The phrase is its line: nothing on the line but the phrase, the marks
/// that make it a heading, an emphasis, a quote, a list item or a banner,
/// and any closing punctuation (#317).
fn alone_on_line(folded: &str, start: usize, end: usize) -> bool {
    opens_line(&folded[..start]) && closes_line(&folded[end..])
}

/// A colon follows the phrase on its line: the first character past `at`
/// that is a letter, a digit, a colon or a line break is the colon. A
/// combining mark belongs to the letter before it.
fn colon_follows(folded: &str, at: usize) -> bool {
    for c in folded[at..].chars() {
        match c {
            ':' => return true,
            c if line_break(c) => return false,
            c if c.is_alphanumeric() && !mark(c) => return false,
            _ => {}
        }
    }
    false
}

/// What the folded text must show of the `new system instructions` phrase
/// beyond its letters, given the byte ranges of its three words (#226, #317):
/// the words are set apart by marks, never by a bar or the end of a clause,
/// and never across a sentence, and it is followed by a colon on its line or
/// is its line.
///
/// A full stop and a space between the words is the phrase's own spelling
/// when there is one after every word or the letters are spelled with them
/// (`new. system. instructions:`, `n. e. w. s. y. s. t. e. m.`): one among
/// the words is two sentences (`Updated. System instructions: see`).
fn directive_shows(folded: &str, words: [(usize, usize); 3]) -> bool {
    let text = |a: usize, b: usize| folded.get(a..b).unwrap_or_default();
    let gaps = [
        unwrap_hyphens(text(words[0].1, words[1].0)),
        unwrap_hyphens(text(words[1].1, words[2].0)),
    ];
    if gaps.iter().any(|g| g.contains('|') || clause_break(g)) {
        return false;
    }
    let breaks = gaps.iter().filter(|g| dot_break(g)).count();
    let spelled = words.iter().any(|&(a, b)| dot_break(text(a, b)));
    if breaks > 0 && breaks < gaps.len() && !spelled {
        return false;
    }
    let (start, end) = (words[0].0, words[2].1);
    colon_follows(folded, end) || alone_on_line(folded, start, end)
}

/// The injection phrases found in the letters-only view of `text`, each
/// placed on the line of the folded text where its first letter stands.
/// The folded text is the text with each removable character made a space,
/// then NFKD and lower case: the view's letters are read out of it, and so
/// are the edges. A match counts only when no ASCII letter of the folded
/// text touches either end of it, a combining mark counting with its
/// letter, so `Danish`, `DEVELOPER_MODE_ONLY` (whose `on` starts `ONLY`) and
/// a phrase inside a camelCase identifier do not hold; a digit, `_`, a
/// letter of another script or any punctuation is an edge, as it is to the
/// raw rules' ASCII `\b`. A phrase with its letters or words separated by
/// punctuation, an invisible character or a mark has edges at its ends all
/// the same. A phrase's tail must show too (#226).
fn letters_findings(field: &str, text: &str) -> Vec<(usize, Finding)> {
    use unicode_normalization::UnicodeNormalization as _;
    // The Hangul fillers are letters that show nothing; every removable
    // character goes before the letters are read. `glue` marks each byte
    // of `folded` that came from an ASCII letter, so a fullwidth `ｘ`, which
    // folds to `x`, is still not the word's own letter.
    let mut folded = String::with_capacity(text.len());
    let mut glue: Vec<bool> = Vec::with_capacity(text.len());
    for c in text.chars() {
        let source = if removable(c) { ' ' } else { c };
        let ascii = source.is_ascii_alphabetic();
        if source.is_ascii() {
            // NFKD and lower case leave ASCII one byte, so skip the iterators.
            folded.push(source.to_ascii_lowercase());
            glue.push(ascii);
            continue;
        }
        for d in std::iter::once(source).nfkd() {
            for l in d.to_lowercase() {
                folded.push(l);
                glue.extend(std::iter::repeat_n(ascii, l.len_utf8()));
            }
        }
    }
    let mut letters = String::with_capacity(folded.len());
    // (offset in `letters`, offset in `folded`) at the start of each run.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for m in LETTER_RUN.find_iter(&folded) {
        runs.push((letters.len(), m.start()));
        letters.push_str(m.as_str());
    }
    // A byte offset of `letters`, as one of `folded`: a start, and an end.
    let start_of = |off: usize| {
        let (at, from) = runs[runs.partition_point(|&(at, _)| at <= off) - 1];
        from + (off - at)
    };
    let end_of = |off: usize| {
        let (at, from) = runs[runs.partition_point(|&(at, _)| at < off) - 1];
        from + (off - at)
    };
    let mut out: Vec<(usize, Finding)> = Vec::new();
    for (rule, re, tail) in LETTERS.iter() {
        if out.iter().any(|(_, f)| f.rule == rule.id) {
            continue;
        }
        let mut from = 0;
        while let Some(caps) = re.captures_at(&letters, from) {
            let m = caps.get(0).expect("a match has its whole");
            let (start, end) = (start_of(m.start()), end_of(m.end()));
            let edges = !glued_before(&folded, &glue, start) && !glued_after(&folded, &glue, end);
            let tail_shows = match tail {
                Tail::None => true,
                // `new`, `system` and `instructions` are the three groups.
                Tail::Directive => {
                    let word = |i: usize| {
                        let g = caps.get(i).expect("each group takes part in a match");
                        (start_of(g.start()), end_of(g.end()))
                    };
                    directive_shows(&folded, [word(1), word(2), word(3)])
                }
            };
            if edges && tail_shows {
                out.push((
                    start,
                    Finding {
                        class: rule.class,
                        rule: rule.id,
                        action: rule.action,
                        field: field.to_owned(),
                        line: line_of(&folded, start),
                        via: "letters",
                        span: (m.start(), m.end()),
                    },
                ));
                break;
            }
            // Look again from the next letter, and the next row of the rule
            // tries the shorter ending a refused match hid: a phrase may
            // start inside the match that was refused.
            from = m.start()
                + letters[m.start()..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
        }
    }
    out
}

fn line_of(text: &str, start: usize) -> usize {
    text.as_bytes()[..start]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Shannon entropy in bits per character.
#[must_use]
pub fn shannon(s: &str) -> f64 {
    let mut ascii = [0_usize; 128];
    let mut freq: BTreeMap<char, usize> = BTreeMap::new();
    let mut n = 0_usize;
    for c in s.chars() {
        if c.is_ascii() {
            ascii[c as usize] += 1;
        } else {
            *freq.entry(c).or_default() += 1;
        }
        n += 1;
    }
    freq.extend(
        ascii
            .iter()
            .zip(0_u8..)
            .filter(|&(&k, _)| k > 0)
            .map(|(&k, c)| (char::from(c), k)),
    );
    if n == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let total = n as f64;
    freq.values()
        .map(|&k| {
            #[allow(clippy::cast_precision_loss)]
            let p = k as f64 / total;
            -p * p.log2()
        })
        .sum()
}

fn placeholder(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    v.starts_with(['$', '%', '<', '{', '*', '@'])
        || PLACEHOLDERS.iter().any(|p| lower.contains(p))
        || lower
            .chars()
            .all(|c| matches!(c, 'x' | '*' | '.' | '-' | '_' | '0'))
}

/// Entropy floors for a key-value secret (d3): 3.0 bits per character
/// under 20 characters, 3.5 from 20. A random 12-character alphanumeric
/// value scores about 3.4; an English word or a snake_case name lacks the
/// digit the rule also asks for.
const ENTROPY_SHORT: f64 = 3.0;
const ENTROPY_LONG: f64 = 3.5;

/// A value ending so names a file, as `GOOGLE_CREDENTIALS = 'sa-123.json'`
/// does: a location, not the secret (d3).
const FILE_SUFFIXES: &[&str] = &[
    ".json", ".yaml", ".yml", ".toml", ".txt", ".pem", ".key", ".p12", ".env",
];

fn windows_path(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Whether a key-value rule's value looks generated rather than written.
fn generated(v: &str) -> bool {
    let v = v.trim_end_matches(['.', ':']);
    let len = v.chars().count();
    if len < 12 || placeholder(v) {
        return false;
    }
    // A digest names its algorithm; a path or URL is a location, not a value.
    let lower = v.to_ascii_lowercase();
    if ["sha1:", "sha256:", "sha512:", "blake3:", "md5:"]
        .iter()
        .any(|p| lower.starts_with(p))
        || v.starts_with(['/', '~'])
        || v.starts_with("./")
        || lower.starts_with("http")
        || lower.starts_with("env:")
        || windows_path(v)
        || FILE_SUFFIXES.iter().any(|x| lower.ends_with(x))
    {
        return false;
    }
    // An identifier written by a person: two or more words of three or more
    // letters in one case between separators (`PROJ-12345-feature-branch`,
    // `postgres-credentials-2024`), which a generated value does not have.
    let words = v
        .split(['-', '_', '.', '/'])
        .filter(|w| {
            w.len() >= 3
                && w.chars().all(|c| c.is_ascii_alphabetic())
                && (w.chars().all(|c| c.is_ascii_lowercase())
                    || w.chars().all(|c| c.is_ascii_uppercase())
                    || w.chars().skip(1).all(|c| c.is_ascii_lowercase()))
        })
        .count();
    if words >= 2 {
        return false;
    }
    let letters = v.chars().any(|c| c.is_ascii_alphabetic());
    let digits = v.chars().any(|c| c.is_ascii_digit());
    let floor = if len < 20 {
        ENTROPY_SHORT
    } else {
        ENTROPY_LONG
    };
    letters && digits && shannon(v) >= floor
}

/// Whether a URL's `user:password` pair carries a real password (d3).
fn real_password(user: &str, pass: &str) -> bool {
    pass.chars().count() >= 4
        && pass != user
        && !placeholder(pass)
        && !PLACEHOLDERS.contains(&pass.to_ascii_lowercase().as_str())
}

/// A long base64 run is a blob when it mixes cases and digits and is dense
/// (d5): 4.5 bits per character, where random base64 scores about 6.
fn blob(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars().any(|c| c.is_ascii_lowercase())
        && s.chars().any(|c| c.is_ascii_digit())
        && shannon(s) >= 4.5
}

fn invisible_format(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{180E}' | '\u{206A}'..='\u{206F}')
}

/// The emoji tag sequences Unicode recommends for general interchange
/// (UTS #51, `emoji-test.txt`, RGI): the subdivision flags of England,
/// Scotland and Wales, each U+1F3F4, its id as tag letters, then the cancel
/// tag U+E007F. No other tag sequence is RGI.
const RGI_TAG_SEQUENCES: [&str; 3] = [
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0077}\u{E006C}\u{E0073}\u{E007F}",
];

/// The invisible-character rules (d5): more than [`INVISIBLE_LIMIT`]
/// zero-width or invisible format characters, or any Unicode tag character
/// outside an RGI emoji tag sequence, read whole (amended 2026-11-10): a
/// flag prefix with any other tag characters after it is a hidden payload,
/// not a flag; and two or more variation selectors in a row
/// (`variation-run`, amended the same day). Joiners (U+200C, U+200D) and the soft hyphen are left alone:
/// scripts and emoji use them.
fn invisible(text: &str) -> Option<((usize, usize), &'static str)> {
    let mut first = None;
    let mut last = 0;
    let mut n = 0;
    let mut selectors = 0;
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        // One variation selector picks a glyph for the character before it;
        // two or more in a row choose nothing and can carry a byte each.
        if matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}') {
            selectors += 1;
            if selectors > 1 {
                return Some(((i, i + c.len_utf8()), "variation-run"));
            }
            continue;
        }
        // A joiner or another removable character between selectors does
        // not end the run: it shows nothing either.
        if !removable(c) {
            selectors = 0;
        }
        if c == '\u{1F3F4}'
            && let Some(flag) = RGI_TAG_SEQUENCES.iter().find(|f| text[i..].starts_with(*f))
        {
            // Past the sequence's tag characters: each is one char.
            chars.nth(flag.chars().count() - 2);
            continue;
        }
        if ('\u{E0000}'..='\u{E007F}').contains(&c) {
            return Some(((i, i + c.len_utf8()), "tag-characters"));
        }
        if invisible_format(c) {
            first.get_or_insert(i);
            last = i + c.len_utf8();
            n += 1;
        }
    }
    (n > INVISIBLE_LIMIT).then(|| ((first.unwrap_or(0), last), "invisible-format"))
}
