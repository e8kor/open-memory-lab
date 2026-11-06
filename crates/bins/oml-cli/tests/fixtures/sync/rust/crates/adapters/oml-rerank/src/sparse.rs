//! The learned sparse encoder behind `oml-app`'s
//! [`SparseEncoder`](oml_app::ports::SparseEncoder) port (ADR-0052 d14):
//! `opensearch-neural-sparse-encoding-doc-v3-distill`, Apache-2.0, a
//! DistilBERT masked-language model run on the CPU through candle.
//!
//! A document is the model card's method: the masked-language logits, the
//! maximum over the tokens, `ln(1 + ln(1 + relu(x)))`, the special tokens
//! zeroed; then the weights below [`SPARSE_FLOOR`] dropped and at most
//! [`SPARSE_MAX_TOKENS`] kept. A query runs no model: the tokenizer's ids,
//! special tokens dropped, each once, weighted by the model's `idf.json`.
//! The network is written out here rather than taken from
//! `candle-transformers`, whose DistilBERT uses the tanh approximation of
//! GELU where the checkpoint was trained with the exact one.

