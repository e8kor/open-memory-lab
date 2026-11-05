//! The false-positive measurement of ADR-0065 d12: scan every `text` of a
//! JSON-lines corpus (one `{"text": …}` per line) and print, per rule, how
//! many documents fired it, with the per-claim time. Prints counts, rule
//! ids and the line numbers of the corpus file; never a matched value.
//!
//! `cargo run --release -p oml-scan --example fp_rate -- <file.jsonl>...`

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader};
use std::time::{Duration, Instant};

use oml_scan::{Options, RuleSet};

fn main() {
    let scanner = RuleSet::new(Options::default());
    for path in std::env::args().skip(1) {
        let file = std::fs::File::open(&path).expect("open corpus");
        let mut docs = 0_u64;
        let mut fired = 0_u64;
        let mut bytes = 0_u64;
        let mut by_rule: BTreeMap<(&str, &str), (u64, Vec<u64>)> = BTreeMap::new();
        let mut elapsed = Duration::ZERO;
        for (n, line) in BufReader::new(file).lines().enumerate() {
            let line = line.expect("read line");
            let doc: serde_json::Value = serde_json::from_str(&line).expect("json line");
            let text = doc["text"].as_str().unwrap_or_default();
            docs += 1;
            bytes += text.len() as u64;
            let start = Instant::now();
            let found = scanner.findings("/body", text);
            elapsed += start.elapsed();
            if !found.is_empty() {
                fired += 1;
            }
            let mut rules: Vec<_> = found.iter().map(|f| (f.action.as_str(), f.rule)).collect();
            rules.dedup();
            for r in rules {
                let e = by_rule.entry(r).or_default();
                e.0 += 1;
                if e.1.len() < 5 {
                    e.1.push(n as u64 + 1);
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let rate = 100.0 * fired as f64 / docs.max(1) as f64;
        println!(
            "{path}: {docs} docs, {bytes} bytes, {fired} with a finding ({rate:.3}%), {:.1} µs per doc",
            elapsed.as_secs_f64() * 1e6 / docs.max(1) as f64
        );
        for ((action, rule), (k, lines)) in by_rule {
            println!("  {action} {rule}: {k} docs (lines {lines:?})");
        }
    }
}
