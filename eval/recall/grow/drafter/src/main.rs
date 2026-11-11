//! Drafts search queries for each memory of `eval/recall/corpus.json` with a
//! local Phi-3 model through candle, on the CPU, with no network.
//!
//!     oml-recall-drafter <model.gguf> <tokenizer.json> <prompt.txt> <corpus.json> <out.jsonl> <seed>
//!
//! The prompt file holds `{text}`, replaced by the memory's text. Each memory
//! is drafted once with temperature 0.7 and top-p 0.9 from the seed `seed +
//! index`, at most 240 new tokens; one JSON line per memory goes to `out`
//! (raw model output, tokens in and out, milliseconds). A memory already in
//! `out` is skipped, so a stopped run resumes. Parsing and every check on
//! the drafts live in `eval/recall/grow.py`.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::Instant;

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_phi3::ModelWeights;
use tokenizers::Tokenizer;

const MAX_NEW: usize = 240;
const TEMPERATURE: f64 = 0.7;
const TOP_P: f64 = 0.9;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 7 {
        return Err("usage: oml-recall-drafter <gguf> <tokenizer.json> <prompt.txt> <corpus.json> <out.jsonl> <seed>".into());
    }
    let (gguf, tok, prompt, corpus, out) = (&a[1], &a[2], &a[3], &a[4], &a[5]);
    let seed: u64 = a[6].parse()?;
    let device = Device::Cpu;
    let template = std::fs::read_to_string(prompt)?;
    let corpus: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(corpus)?)?;
    let tokenizer = Tokenizer::from_file(tok).map_err(|e| e.to_string())?;
    let end = ["<|end|>", "<|endoftext|>"]
        .iter()
        .filter_map(|t| tokenizer.token_to_id(t))
        .collect::<HashSet<u32>>();

    let mut done = HashSet::new();
    if let Ok(f) = File::open(out) {
        for line in BufReader::new(f).lines() {
            let v: serde_json::Value = serde_json::from_str(&line?)?;
            done.insert(v["memory_id"].as_str().unwrap_or_default().to_string());
        }
    }
    let mut sink = OpenOptions::new().create(true).append(true).open(out)?;

    let memories = corpus["memories"]
        .as_array()
        .ok_or("corpus has no memories")?;
    for (i, m) in memories.iter().enumerate() {
        let id = m["id"].as_str().ok_or("memory without id")?;
        if done.contains(id) {
            continue;
        }
        let text = m["text"].as_str().ok_or("memory without text")?;
        let user = template.replace("{text}", text);
        let chat = format!("<|user|>\n{}<|end|>\n<|assistant|>\n", user.trim_end());
        let ids = tokenizer
            .encode(chat, true)
            .map_err(|e| e.to_string())?
            .get_ids()
            .to_vec();
        // A fresh model per memory: no cache carries over between drafts.
        let mut file = File::open(gguf)?;
        let content = gguf_file::Content::read(&mut file)?;
        let mut model = ModelWeights::from_gguf(false, content, &mut file, &device)?;
        let s = seed + i as u64;
        let mut lp = LogitsProcessor::new(s, Some(TEMPERATURE), Some(TOP_P));
        let t0 = Instant::now();
        let input = Tensor::new(ids.as_slice(), &device)?.unsqueeze(0)?;
        let logits = model.forward(&input, 0)?.squeeze(0)?;
        let mut next = lp.sample(&logits)?;
        let mut outs = Vec::new();
        while !end.contains(&next) && outs.len() < MAX_NEW {
            outs.push(next);
            let input = Tensor::new(&[next], &device)?.unsqueeze(0)?;
            let logits = model
                .forward(&input, ids.len() + outs.len() - 1)?
                .squeeze(0)?;
            next = lp.sample(&logits)?;
        }
        let raw = tokenizer.decode(&outs, true).map_err(|e| e.to_string())?;
        let line = serde_json::json!({
            "memory_id": id, "seed": s, "raw": raw, "tokens_in": ids.len(),
            "tokens_out": outs.len(), "ms": t0.elapsed().as_millis() as u64,
        });
        writeln!(sink, "{line}")?;
        sink.flush()?;
        eprintln!(
            "{id}: {} in, {} out, {} ms",
            ids.len(),
            outs.len(),
            t0.elapsed().as_millis()
        );
    }
    Ok(())
}
