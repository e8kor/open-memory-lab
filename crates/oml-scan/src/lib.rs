//! The deterministic content scan, rule set `oml-scan/1` (ADR-0065).
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

use regex::{Regex, RegexSet, RegexSetBuilder};

pub use path::secret_path;
pub use rules::Action;
use rules::{Check, PLACEHOLDERS, RULES, Rule};

/// The rule-set version every verdict is journalled under (ADR-0065 d1).
pub const RULESET: &str = "oml-scan/1";

/// The largest text one scan reads: 1 MiB. A longer field is vetoed as
/// `format.scan_bound` rather than scanned in part (d9).
pub const MAX_SCAN_BYTES: usize = 1 << 20;

/// More invisible format characters than this in one field is a hold (d5).
pub const INVISIBLE_LIMIT: usize = 3;

/// The rule set's lazy DFA cache, per thread that scans.
const DFA_CACHE_BYTES: usize = 16 << 20;

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

struct Compiled {
    set: RegexSet,
    one: Vec<Regex>,
}

static COMPILED: LazyLock<Compiled> = LazyLock::new(|| {
    let patterns = RULES.iter().map(|r| r.pattern);
    Compiled {
        // A lazy DFA cache large enough that a long field never thrashes it
        // back to the slower engine (d9).
        set: RegexSetBuilder::new(patterns)
            .dfa_size_limit(DFA_CACHE_BYTES)
            .build()
            .expect("the oml-scan/1 patterns compile"),
        one: RULES
            .iter()
            .map(|r| Regex::new(r.pattern).expect("the oml-scan/1 patterns compile"))
            .collect(),
    }
});

/// Every rule id the set can report, with its class and action: the closed
/// label set of the findings metric (d11).
#[must_use]
pub fn rule_ids() -> Vec<(&'static str, &'static str, Action)> {
    let mut out: Vec<_> = RULES.iter().map(|r| (r.class, r.id, r.action)).collect();
    out.push(("unicode.invisible", "invisible-format", Action::Hold));
    out.push(("unicode.invisible", "tag-characters", Action::Hold));
    out.push(("format.scan_bound", "scan-bound", Action::Veto));
    out
}

/// The scanner: the compiled `oml-scan/1` set, the deployment's options,
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
        let finding = |rule: &Rule, start: usize| Finding {
            class: rule.class,
            rule: rule.id,
            action: rule.action,
            field: field.to_owned(),
            line: line_of(text, start),
        };
        if text.len() > MAX_SCAN_BYTES {
            return vec![Finding {
                class: "format.scan_bound",
                rule: "scan-bound",
                action: Action::Veto,
                field: field.to_owned(),
                line: 1,
            }];
        }
        let compiled = &*COMPILED;
        let mut out: Vec<(usize, Finding)> = Vec::new();
        for i in compiled.set.matches(text) {
            let rule = &RULES[i];
            if rule.pii && !self.options.pii {
                continue;
            }
            if let Some(start) = first_match(rule, &compiled.one[i], text) {
                out.push((start, finding(rule, start)));
            }
        }
        if let Some((start, rule)) = invisible(text) {
            out.push((
                start,
                Finding {
                    class: "unicode.invisible",
                    rule,
                    action: Action::Hold,
                    field: field.to_owned(),
                    line: line_of(text, start),
                },
            ));
        }
        out.sort_by_key(|(start, f)| (*start, f.rule));
        out.into_iter().map(|(_, f)| f).collect()
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
        Check::Blob => re
            .find_iter(text)
            .find(|m| blob(m.as_str()))
            .map(|m| m.start()),
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
    let mut freq: BTreeMap<char, usize> = BTreeMap::new();
    let mut n = 0_usize;
    for c in s.chars() {
        *freq.entry(c).or_default() += 1;
        n += 1;
    }
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

/// The invisible-character rules (d5): more than [`INVISIBLE_LIMIT`]
/// zero-width or invisible format characters, or any Unicode tag character
/// outside an emoji flag sequence (which opens with U+1F3F4). Joiners
/// (U+200C, U+200D) and the soft hyphen are left alone: scripts and emoji
/// use them.
fn invisible(text: &str) -> Option<(usize, &'static str)> {
    let mut first = None;
    let mut n = 0;
    let mut prev_flag = false;
    for (i, c) in text.char_indices() {
        if ('\u{E0000}'..='\u{E007F}').contains(&c) {
            if !prev_flag {
                return Some((i, "tag-characters"));
            }
            continue;
        }
        prev_flag = c == '\u{1F3F4}';
        if invisible_format(c) {
            first.get_or_insert(i);
            n += 1;
        }
    }
    (n > INVISIBLE_LIMIT).then(|| (first.unwrap_or(0), "invisible-format"))
}
