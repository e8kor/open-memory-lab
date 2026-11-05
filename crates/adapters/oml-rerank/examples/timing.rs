//! Times the cross-encoder on the calling thread (ADR-0053 d4): `n` pairs
//! of a query and a passage of `words` words, `runs` times, printing the
//! median and p95 in milliseconds.
//!
//!     cargo run --release -p oml-rerank --example timing -- <model-dir> <n> <words> <runs>

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(&args[1]);
    let n: usize = args[2].parse().expect("n");
    let words: usize = args[3].parse().expect("words");
    let runs: usize = args[4].parse().expect("runs");
    let model = oml_rerank::CrossEncoder::load(
        &dir,
        &oml_rerank::MINILM_L6,
        oml_rerank::Limits {
            max_concurrent: 1,
            timeout: std::time::Duration::from_secs(60),
        },
    )
    .expect("model");
    let passage = |i: usize| -> String {
        (0..words)
            .map(|w| format!("word{}", (i * 7 + w) % 997))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let passages: Vec<String> = (0..n).map(passage).collect();
    let mut ms: Vec<f64> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            model
                .score_now("which port does postgres listen on", &passages)
                .expect("scores");
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    println!(
        "n {n} words {words}: p50 {:.1} ms p95 {:.1} ms",
        ms[runs / 2],
        ms[(runs * 95 / 100).min(runs - 1)]
    );
}
