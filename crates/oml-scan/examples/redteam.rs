//! The red-team report of ADR-0074 d15: per class family precision,
//! recall and the twins' false-positive rate, held-out recall with Wilson
//! 95 % intervals, recall per language and per mutation, under the
//! compiled rule set. Prints ids, counts and rule ids; never a sample's
//! text. `tests/corpus/baseline.txt` is this report with `--verdicts`, and
//! `tests/redteam.rs` fails when the two differ.
//!
//! `cargo run --release -p oml-scan --example redteam [-- --verdicts] [-- --docs]`
//!
//! `--docs` adds the benign security prose of this repository: every
//! paragraph of `docs/architecture/security.md` and `docs/adr/*.md`, and
//! the share of them flagged (d15: at most 1 % held).

#[path = "../tests/corpus/harness.rs"]
mod harness;

use std::path::Path;

use oml_scan::{Options, RuleSet};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let samples = harness::load();
    print!(
        "{}",
        harness::report(&samples, args.iter().any(|a| a == "--verdicts"))
    );
    if args.iter().any(|a| a == "--docs") {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = vec![root.join("docs/architecture/security.md")];
        let mut adrs: Vec<_> = std::fs::read_dir(root.join("docs/adr"))
            .expect("docs/adr")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        adrs.sort();
        files.extend(adrs);
        let scanner = RuleSet::new(Options::default());
        let (mut n, mut flagged) = (0_usize, 0_usize);
        for file in files {
            let body = std::fs::read_to_string(&file).expect("read a doc");
            for para in body.split("\n\n").filter(|p| !p.trim().is_empty()) {
                n += 1;
                if harness::verdict(&scanner, &[("/body", para)]) != "pass" {
                    flagged += 1;
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let rate = 100.0 * flagged as f64 / n.max(1) as f64;
        println!(
            "\nbenign security prose of this repository (docs/architecture/security.md, docs/adr/*.md): {n} paragraphs, {flagged} flagged ({rate:.2}%)"
        );
    }
}
