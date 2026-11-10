//! The deterministic content scan, rule set `oml-scan/1.1` (ADR-0065; `/1` before the 2026-11-10 amendment).
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
use rules::{Check, PLACEHOLDERS, RULES, Rule};

/// The rule-set version every verdict is journalled under (ADR-0065 d1).
pub const RULESET: &str = "oml-scan/1.1";

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
        .map(|r| Regex::new(r.pattern).expect("the oml-scan/1.1 patterns compile"))
        .collect()
});

/// Every rule id the set can report, with its class and action: the closed
/// label set of the findings metric (d11).
#[must_use]
pub fn rule_ids() -> Vec<(&'static str, &'static str, Action)> {
    let mut out: Vec<_> = RULES.iter().map(|r| (r.class, r.id, r.action)).collect();
    out.push(("secret.assignment", "aws-secret-near-id", Action::Veto));
    out.push(("encoding.base64_blob", "base64-blob", Action::Hold));
    out.push(("unicode.invisible", "invisible-format", Action::Hold));
    out.push(("unicode.invisible", "tag-characters", Action::Hold));
    out.push(("format.scan_bound", "scan-bound", Action::Veto));
    out
}

/// The scanner: the compiled `oml-scan/1.1` set, the deployment's options,
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
            }];
        }
        // The order is fixed (d14, amended 2026-11-10): first every pattern
        // and run rule over the text as written; then the invisible-character
        // rules over the text as written, since the view removes what they
        // count; then every pattern and run rule again over the normalised
        // view, veto and hold alike, keeping each rule the text itself did
        // not already report. A zero-width character or a fullwidth form
        // inside an injection phrase hides it from the first pass, never
        // from the third.
        let mut out = self.matched(field, text);
        if let Some((start, rule)) = invisible(text) {
            out.push((
                start,
                code_finding("unicode.invisible", rule, Action::Hold, field, text, start),
            ));
        }
        if let Some(view) = normalised(text) {
            for (start, f) in self.matched(field, &view) {
                if !out.iter().any(|(_, g)| g.rule == f.rule) {
                    out.push((start, f));
                }
            }
        }
        out.sort_by_key(|(start, f)| (*start, f.rule));
        out.into_iter().map(|(_, f)| f).collect()
    }

    /// The pattern and run rules over one text, with each finding's start.
    fn matched(&self, field: &str, text: &str) -> Vec<(usize, Finding)> {
        let mut out: Vec<(usize, Finding)> = Vec::new();
        for (rule, re) in RULES.iter().zip(COMPILED.iter()) {
            // The personal-data rules do not run while they are off (d6).
            if rule.pii && !self.options.pii {
                continue;
            }
            if let Some(start) = first_match(rule, re, text) {
                out.push((
                    start,
                    code_finding(rule.class, rule.id, rule.action, field, text, start),
                ));
            }
        }
        if let Some(start) = base64_run(text) {
            out.push((
                start,
                code_finding(
                    "encoding.base64_blob",
                    "base64-blob",
                    Action::Hold,
                    field,
                    text,
                    start,
                ),
            ));
        }
        if out.iter().any(|(_, f)| f.rule == "aws-access-key-id")
            && let Some(start) = aws_secret_near_id(text)
        {
            out.push((
                start,
                code_finding(
                    "secret.assignment",
                    "aws-secret-near-id",
                    Action::Veto,
                    field,
                    text,
                    start,
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

/// Where the rule's first real finding starts, after its second look.
fn first_match(rule: &Rule, re: &Regex, text: &str) -> Option<usize> {
    match rule.check {
        Check::None => re.find(text).map(|m| m.start()),
        Check::Entropy => re
            .captures_iter(text)
            .find(|c| c.get(1).is_some_and(|v| generated(v.as_str())))
            .and_then(|c| c.get(0))
            .map(|m| m.start()),
        Check::Password => re
            .captures_iter(text)
            .find(|c| match (c.get(1), c.get(2)) {
                (Some(user), Some(pass)) => real_password(user.as_str(), pass.as_str()),
                _ => false,
            })
            .and_then(|c| c.get(0))
            .map(|m| m.start()),
    }
}

fn code_finding(
    class: &'static str,
    rule: &'static str,
    action: Action,
    field: &str,
    text: &str,
    start: usize,
) -> Finding {
    Finding {
        class,
        rule,
        action,
        field: field.to_owned(),
        line: line_of(text, start),
    }
}

/// The shortest base64 run the blob rule reads (d5).
const BLOB_MIN: usize = 400;

/// The first run of 400 or more base64 characters that passes the blob
/// test, counted by hand: one pass, no regex state (d9).
fn base64_run(text: &str) -> Option<usize> {
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
            return Some(start);
        }
    }
    None
}

/// A 40-character base64 value within 200 bytes after an AWS access key
/// id, with the entropy test: the secret half of a pair pasted with no key
/// name (d3).
fn aws_secret_near_id(text: &str) -> Option<usize> {
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
                return Some(start);
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

/// The text with what hides a token or a phrase undone, or `None` when it
/// is already that text: removable characters gone, NBSP a space, NFKC
/// (d14). Not case-folded: the hold patterns match either case already,
/// and folding would hide an upper-case key from the veto patterns.
fn normalised(text: &str) -> Option<String> {
    use unicode_normalization::UnicodeNormalization as _;
    use unicode_normalization::{IsNormalized, is_nfkc_quick};
    let plain = text.is_ascii()
        || (!text.chars().any(|c| removable(c) || c == '\u{00A0}')
            && is_nfkc_quick(text.chars()) == IsNormalized::Yes);
    if plain {
        return None;
    }
    let view: String = text
        .chars()
        .filter(|&c| !removable(c))
        .map(|c| if c == '\u{00A0}' { ' ' } else { c })
        .nfkc()
        .collect();
    (view != text).then_some(view)
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
/// not a flag. Joiners (U+200C, U+200D) and the soft hyphen are left alone:
/// scripts and emoji use them.
fn invisible(text: &str) -> Option<(usize, &'static str)> {
    let mut first = None;
    let mut n = 0;
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '\u{1F3F4}'
            && let Some(flag) = RGI_TAG_SEQUENCES.iter().find(|f| text[i..].starts_with(*f))
        {
            // Past the sequence's tag characters: each is one char.
            chars.nth(flag.chars().count() - 2);
            continue;
        }
        if ('\u{E0000}'..='\u{E007F}').contains(&c) {
            return Some((i, "tag-characters"));
        }
        if invisible_format(c) {
            first.get_or_insert(i);
            n += 1;
        }
    }
    (n > INVISIBLE_LIMIT).then(|| (first.unwrap_or(0), "invisible-format"))
}
