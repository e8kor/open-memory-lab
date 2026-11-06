//! The local cross-encoder behind `oml-app`'s [`Reranker`](oml_app::ports::Reranker) port (ADR-0053).
//!
//! A BERT-class sequence classifier (the `ms-marco-MiniLM` cross-encoders)
//! runs on the CPU through candle, pure Rust (ADR-0006): the query and a
//! passage go in as one pair, the classifier's logit comes out, and the
//! port's relevance is its logistic, a probability in `[0, 1]` that reads
//! the same way on every query, so a relevance floor can be one number
//! (ADR-0053 d6).
//!
//! The model is three files in one directory, `config.json`,
//! `tokenizer.json` and `model.safetensors`, each checked against the
//! SHA-256 this build pins for the model before a byte of it is parsed
//! ([`ModelSpec`]). A deployment either places them there itself, which is
//! how an offline install works, or lets the lead fetch the missing ones
//! from the pinned revision at start through [`fetch_missing`], whose
//! transport the composition supplies (ADR-0053 d3).
//!
//! Inference is CPU work, so it runs on tokio's blocking pool, under a
//! semaphore of `max_concurrent` slots and a deadline: a call that waits past
//! the deadline for a slot answers [`RerankError::Busy`](oml_app::ports::RerankError::Busy), one whose model
//! runs past it [`RerankError::Timeout`](oml_app::ports::RerankError::Timeout), and the core then serves the first
//! stage's order (ADR-0053 d5). A late inference keeps its slot until it
//! ends, so the cap bounds the CPU actually in use.

#[cfg(feature = "model")]
mod model;
mod sparse;
