//! The learned sparse encoder over a file of texts, for the scale bench and
//! the encode throughput figure (ADR-0052 d14, #104): one JSON line per
//! text on stdin, `{"text": …}`, and one JSON line out per text, its
//! postings `[[token, weight, in_text]…]` (`docs`) or its query terms
//! `[[token, weight, piece]…]` (`queries`). The throughput goes to stderr.
//!
//! ```sh
//! cargo run --release -p oml-rerank --example sparse_encode -- <model dir> <threads> docs|queries
//! ```

use std::io::{BufRead as _, Write as _};
use std::time::Instant;

use oml_app::ports::SparseEncoder as _;
use oml_rerank::{DOC_V3_DISTILL, SparseModel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, dir, threads, mode] = args.as_slice() else {
        return Err("usage: sparse_encode <model dir> <threads> docs|queries".into());
    };
    let model = SparseModel::load(std::path::Path::new(dir), &DOC_V3_DISTILL, threads.parse()?)?;
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout());
    let (mut n, mut chars) = (0_u64, 0_u64);
    let started = Instant::now();
    for line in stdin.lock().lines() {
        let v: serde_json::Value = serde_json::from_str(&line?)?;
        let text = v["text"].as_str().unwrap_or_default();
        chars += text.chars().count() as u64;
        let row = if mode == "queries" {
            serde_json::json!(
                model
                    .encode_query(text)
                    .into_iter()
                    .map(|t| serde_json::json!([t.token_id, t.weight, t.piece]))
                    .collect::<Vec<_>>()
            )
        } else {
            serde_json::json!(
                model
                    .document(text)?
                    .into_iter()
                    .map(|p| serde_json::json!([p.token_id, p.weight, p.in_text]))
                    .collect::<Vec<_>>()
            )
        };
        writeln!(out, "{row}")?;
        n += 1;
    }
    out.flush()?;
    let secs = started.elapsed().as_secs_f64();
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let (rate, mean) = (n as f64 / secs, chars as f64 / n.max(1) as f64);
    eprintln!(
        "{{\"texts\": {n}, \"seconds\": {secs:.1}, \"per_second\": {rate:.1}, \"mean_chars\": {mean:.0}, \"threads\": {threads}}}"
    );
    Ok(())
}
