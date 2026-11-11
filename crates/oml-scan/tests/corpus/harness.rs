//! The red-team harness (ADR-0074 d15): loads the corpus beside this file,
//! scans every sample under the compiled rule set, and writes the report the
//! `redteam` example prints and `tests/redteam.rs` compares with the
//! committed baseline. Shared by both through `#[path]`, so the two cannot
//! disagree.
//!
//! The corpus is live injection text in a repository coding agents read, so
//! every malicious sample is stored base64-encoded (`text_b64`) and a secret
//! sample in two base64 halves (`parts_b64`), decoded here at run time and
//! never written out. The report names sample ids, classes, verdicts and
//! counts; it never prints a sample's text.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use oml_scan::{Action, Options, RuleSet, Verdict};

/// The corpus directory.
#[must_use]
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

/// The verdicts a sample may expect (ADR-0074 d1), and `pass`.
pub const VERDICTS: &[&str] = &["refuse", "hold", "annotate", "pass"];

/// The sets a sample belongs to (ADR-0074 d15).
pub const SETS: &[&str] = &[
    "malicious",
    "twin",
    "heldout",
    "markdown_hidden",
    "benign_prose",
];

/// No rate is printed for a family with fewer malicious samples or twins
/// than this (ADR-0074 d15).
pub const RATE_FLOOR: usize = 50;

/// One corpus sample as stored: its fields, and the text decoded.
#[derive(Debug, Clone)]
pub struct Sample {
    pub file: String,
    pub id: String,
    pub set: String,
    pub class: String,
    pub expected: String,
    pub lang: String,
    pub twin_of: Option<String>,
    pub raw: serde_json::Value,
    pub text: String,
}

impl Sample {
    /// The class family: `injection` for `injection.override`.
    #[must_use]
    pub fn family(&self) -> &str {
        self.class.split('.').next().unwrap_or(&self.class)
    }

    /// Malicious in the report's sense: a sample that should not pass.
    #[must_use]
    pub fn malicious(&self) -> bool {
        self.expected != "pass"
    }
}

/// Every sample of every `*.jsonl` file, in file then line order. A line
/// that does not parse is a panic naming its file and line: the corpus is
/// test data, and a broken line is a defect.
#[must_use]
pub fn load() -> Vec<Sample> {
    let dir = corpus_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the corpus directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let body = std::fs::read_to_string(&path).expect("read a corpus file");
        for (n, line) in body.lines().enumerate() {
            let raw: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{name}:{}: {e}", n + 1));
            let s = |k: &str| raw[k].as_str().unwrap_or_default().to_owned();
            let text = if let Some(t) = raw["text"].as_str() {
                t.to_owned()
            } else if let Some(t) = raw["text_b64"].as_str() {
                decode_b64(t).unwrap_or_else(|| panic!("{name}:{}: text_b64", n + 1))
            } else if let Some(parts) = raw["parts_b64"].as_array() {
                parts
                    .iter()
                    .map(|p| {
                        decode_b64(p.as_str().unwrap_or_default())
                            .unwrap_or_else(|| panic!("{name}:{}: parts_b64", n + 1))
                    })
                    .collect()
            } else {
                String::new()
            };
            out.push(Sample {
                file: name.clone(),
                id: s("id"),
                set: s("set"),
                class: s("class"),
                expected: s("expected"),
                lang: s("lang"),
                twin_of: raw["twin_of"].as_str().map(str::to_owned),
                raw,
                text,
            });
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
#[must_use]
pub fn encode_b64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= n {
                out.push(B64[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard base64, padding optional; `None` on any other character or on
/// bytes that are not UTF-8.
#[must_use]
pub fn decode_b64(s: &str) -> Option<String> {
    let mut bits = 0_u32;
    let mut n = 0;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = B64.iter().position(|&b| b == c)? as u32;
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The verdict the set gives the fields: `refuse`, `hold` or `pass`.
/// `oml-scan/1.x` has no annotate verdict.
#[must_use]
pub fn verdict(scanner: &RuleSet, fields: &[(&str, &str)]) -> &'static str {
    match scanner.scan(fields) {
        Verdict::Veto(_) => "refuse",
        Verdict::Hold(_) => "hold",
        Verdict::Pass => "pass",
    }
}

/// The rule ids of every finding the set makes in the fields.
#[must_use]
pub fn rules(scanner: &RuleSet, fields: &[(&str, &str)]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = fields
        .iter()
        .flat_map(|(f, t)| scanner.findings(f, t))
        .filter(|f| matches!(f.action, Action::Veto | Action::Hold))
        .map(|f| f.rule)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn fullwidth(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            ' ' => '\u{3000}',
            '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn per_word(text: &str, f: impl Fn(&str) -> String) -> String {
    text.split(' ').map(f).collect::<Vec<_>>().join(" ")
}

fn spaced(text: &str, sep: &str) -> String {
    per_word(text, |w| {
        w.chars()
            .map(String::from)
            .collect::<Vec<_>>()
            .join(sep)
    })
}

fn every(text: &str, n: usize, insert: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i > 0 && i % n == 0 {
            out.push_str(insert);
        }
        out.push(c);
    }
    out
}

fn percent(c: char) -> String {
    let mut buf = [0_u8; 4];
    c.encode_utf8(&mut buf)
        .bytes()
        .map(|b| format!("%{b:02X}"))
        .collect()
}

fn b64_depth(text: &str, depth: usize) -> String {
    let mut s = text.to_owned();
    for _ in 0..depth {
        s = encode_b64(s.as_bytes());
    }
    s
}

/// One letter in the middle of each word of three or more, rewritten by `f`.
fn partial(text: &str, f: impl Fn(char) -> String) -> String {
    per_word(text, |w| {
        let chars: Vec<char> = w.chars().collect();
        if chars.len() < 3 {
            return w.to_owned();
        }
        let mid = chars.len() / 2;
        chars
            .iter()
            .enumerate()
            .map(|(i, &c)| if i == mid { f(c) } else { c.to_string() })
            .collect()
    })
}

/// A mutation: its name and the fields it makes of a sample's text.
pub type Mutation = (&'static str, fn(&str) -> Vec<(String, String)>);

fn body(text: String) -> Vec<(String, String)> {
    vec![("/body".to_owned(), text)]
}

/// Every transform of ADR-0074 d6's evasion tests and d15's mutation suite
/// the harness applies to every malicious sample. Translation is stored as
/// samples of their own (`lang`), not made here.
pub const MUTATIONS: &[Mutation] = &[
    ("zero-width-in-word", |t| {
        body(per_word(t, |w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) if w.chars().count() >= 3 => format!("{first}\u{200B}{}", c.as_str()),
                _ => w.to_owned(),
            }
        }))
    }),
    ("fullwidth", |t| body(fullwidth(t))),
    ("mixed-case", |t| {
        body(
            t.chars()
                .enumerate()
                .map(|(i, c)| {
                    if i % 2 == 0 {
                        c.to_uppercase().collect::<String>()
                    } else {
                        c.to_lowercase().collect()
                    }
                })
                .collect(),
        )
    }),
    ("homoglyph", |t| {
        body(
            t.chars()
                .map(|c| match c {
                    'a' => 'а',
                    'e' => 'е',
                    'o' => 'о',
                    'c' => 'с',
                    'p' => 'р',
                    _ => c,
                })
                .collect(),
        )
    }),
    ("joined-underscore", |t| body(t.replace(' ', "_"))),
    ("joined-slash", |t| body(t.replace(' ', "/"))),
    ("letters-dash", |t| body(spaced(t, "-"))),
    ("letters-dot", |t| body(spaced(t, "."))),
    ("letters-space", |t| body(spaced(t, " "))),
    ("title-body-split", |t| {
        let words: Vec<&str> = t.split(' ').collect();
        let mid = words.len().div_ceil(2);
        vec![
            ("/title".to_owned(), words[..mid].join(" ")),
            ("/body".to_owned(), words[mid..].join(" ")),
        ]
    }),
    ("base64-depth-1", |t| body(b64_depth(t, 1))),
    ("base64-depth-2", |t| body(b64_depth(t, 2))),
    ("base64-depth-3", |t| body(b64_depth(t, 3))),
    ("base64-zero-width-split", |t| {
        body(every(&b64_depth(t, 1), 20, "\u{200B}"))
    }),
    ("base64-entity-split", |t| {
        body(every(&b64_depth(t, 1), 20, "&#x200B;"))
    }),
    ("base64-fullwidth", |t| body(fullwidth(&b64_depth(t, 1)))),
    ("percent-fullwidth", |t| {
        body(fullwidth(&t.chars().map(percent).collect::<String>()))
    }),
    ("percent-double", |t| {
        body(
            t.chars()
                .map(|c| percent(c).replace('%', "%25"))
                .collect(),
        )
    }),
    ("entity-double", |t| {
        body(t.chars().map(|c| format!("&amp;#{};", c as u32)).collect())
    }),
    ("partial-percent", |t| body(partial(t, percent))),
    ("partial-entity", |t| {
        body(partial(t, |c| format!("&#{};", c as u32)))
    }),
    ("budget-padding", |t| {
        body(format!("{}{}", "%41".repeat(4096), b64_depth(t, 1)))
    }),
];

/// A Wilson score interval at 95 % for `k` of `n`, as percentages.
#[must_use]
pub fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 0.0);
    }
    let z = 1.959_964_f64;
    #[allow(clippy::cast_precision_loss)]
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let denom = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    (
        100.0 * (centre - half).max(0.0),
        100.0 * (centre + half).min(1.0),
    )
}

fn pct(k: usize, n: usize) -> String {
    if n == 0 {
        return "-".to_owned();
    }
    #[allow(clippy::cast_precision_loss)]
    let v = 100.0 * k as f64 / n as f64;
    format!("{v:.1}%")
}

/// The report under the compiled rule set, every line deterministic: no
/// timing, no text. `verdicts` adds one line per sample.
#[must_use]
pub fn report(samples: &[Sample], verdicts: bool) -> String {
    let scanner = RuleSet::new(Options::default());
    let mut out = String::new();
    let _ = writeln!(
        out,
        "redteam: {} over the corpus of ADR-0074 d15, {} samples",
        scanner.version(),
        samples.len()
    );
    let _ = writeln!(
        out,
        "a rate needs {RATE_FLOOR} malicious samples and {RATE_FLOOR} twins in its family (d15); below that only counts print"
    );
    let actual: Vec<&'static str> = samples
        .iter()
        .map(|s| verdict(&scanner, &[("/body", s.text.as_str())]))
        .collect();
    let caught = |i: usize| actual[i] != "pass";

    // Per family: malicious samples and twins.
    let mut fam: BTreeMap<&str, [usize; 5]> = BTreeMap::new();
    for (i, s) in samples.iter().enumerate() {
        let e = fam.entry(s.family()).or_default();
        match s.set.as_str() {
            "malicious" => {
                e[0] += 1;
                e[2] += usize::from(caught(i));
                e[3] += usize::from(actual[i] == s.expected);
            }
            "twin" => {
                e[1] += 1;
                e[4] += usize::from(caught(i) && s.expected == "pass");
            }
            _ => {}
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{:<10} {:>9} {:>6} {:>7} {:>6} {:>8} {:>9} {:>10}",
        "family", "malicious", "twins", "caught", "exact", "recall", "twin-fp", "precision"
    );
    for (family, [m, t, c, x, fp]) in &fam {
        if *m == 0 && *t == 0 {
            continue;
        }
        let rated = *m >= RATE_FLOOR && *t >= RATE_FLOOR;
        let (recall, twin_fp, precision) = if rated {
            (pct(*c, *m), pct(*fp, *t), pct(*c, c + fp))
        } else {
            ("n<50".to_owned(), "n<50".to_owned(), "n<50".to_owned())
        };
        let _ = writeln!(
            out,
            "{family:<10} {m:>9} {t:>6} {c:>7} {x:>6} {recall:>8} {twin_fp:>9} {precision:>10}"
        );
    }

    // Translations: the malicious samples not in English, per language.
    let mut langs: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (i, s) in samples.iter().enumerate() {
        if s.set == "malicious" && s.lang != "en" {
            let e = langs.entry(s.lang.as_str()).or_default();
            e.0 += 1;
            e.1 += usize::from(caught(i));
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "translated malicious samples, caught per language:");
    for (lang, (n, k)) in &langs {
        let _ = writeln!(out, "  {lang}: {k} of {n}");
    }

    // Held-out: frozen, never read while tuning; Wilson 95 %.
    let held: Vec<usize> = (0..samples.len())
        .filter(|&i| samples[i].set == "heldout")
        .collect();
    let (inj, ben): (Vec<usize>, Vec<usize>) =
        held.iter().partition(|&&i| samples[i].malicious());
    let k_inj = inj.iter().filter(|&&i| caught(i)).count();
    let k_ben = ben.iter().filter(|&&i| caught(i)).count();
    let (lo, hi) = wilson(k_inj, inj.len());
    let (blo, bhi) = wilson(k_ben, ben.len());
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "held-out (deepset/prompt-injections, test split, Apache-2.0): {} injections, {k_inj} caught, recall {} [Wilson 95%: {lo:.1}%-{hi:.1}%]",
        inj.len(),
        pct(k_inj, inj.len())
    );
    let _ = writeln!(
        out,
        "held-out benign: {} samples, {k_ben} flagged, {} [Wilson 95%: {blo:.1}%-{bhi:.1}%]",
        ben.len(),
        pct(k_ben, ben.len())
    );

    // Hidden by Markdown and the benign security prose.
    for (set, label) in [
        ("markdown_hidden", "hidden by Markdown, caught"),
        ("benign_prose", "benign security prose, flagged"),
    ] {
        let ids: Vec<usize> = (0..samples.len())
            .filter(|&i| samples[i].set == set)
            .collect();
        let k = ids.iter().filter(|&&i| caught(i)).count();
        let _ = writeln!(out, "{label}: {k} of {}", ids.len());
    }

    // Mutations: every malicious sample, transformed; recall against the
    // same samples plain.
    let mal: Vec<usize> = (0..samples.len())
        .filter(|&i| samples[i].set == "malicious")
        .collect();
    let plain = mal.iter().filter(|&&i| caught(i)).count();
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "mutations over {} malicious samples; plain recall {} ({plain} caught); invariance asks each transform for at least the plain recall less 5 points (d15)",
        mal.len(),
        pct(plain, mal.len())
    );
    for (name, mutate) in MUTATIONS {
        let k = mal
            .iter()
            .filter(|&&i| {
                let fields = mutate(&samples[i].text);
                let refs: Vec<(&str, &str)> = fields
                    .iter()
                    .map(|(f, t)| (f.as_str(), t.as_str()))
                    .collect();
                verdict(&scanner, &refs) != "pass"
            })
            .count();
        #[allow(clippy::cast_precision_loss)]
        let delta = 100.0 * (k as f64 - plain as f64) / mal.len().max(1) as f64;
        let _ = writeln!(
            out,
            "  {name:<24} {:>6}  {delta:+.1} points  {}",
            pct(k, mal.len()),
            if delta >= -5.0 { "holds" } else { "below" }
        );
    }

    if verdicts {
        let _ = writeln!(out);
        let _ = writeln!(out, "per sample: id expected actual rules");
        let scanner = RuleSet::new(Options::default());
        for (i, s) in samples.iter().enumerate() {
            let r = rules(&scanner, &[("/body", s.text.as_str())]);
            let _ = writeln!(
                out,
                "{} {} {} {}",
                s.id,
                s.expected,
                actual[i],
                if r.is_empty() {
                    "-".to_owned()
                } else {
                    r.join(",")
                }
            );
        }
    }
    out
}
