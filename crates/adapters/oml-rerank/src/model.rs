//! The cross-encoder network and its served wrapper, behind the `model`
//! feature (ADR-0053 d2, d5).

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, VarBuilder, linear};
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{Encoding, Tokenizer, TruncationDirection};
use tokio::sync::Semaphore;

use oml_app::ports::{RerankError, RerankFuture, Reranker};

use crate::{
    BATCH, Limits, LoadError, MAX_TOKENS, MODEL_FILES, ModelSpec, QUERY_SHARE, cut_passage,
};

/// Reads `dir/file` and checks it is the file `spec` pins.
fn read_checked(dir: &Path, spec: &ModelSpec, i: usize) -> Result<Vec<u8>, LoadError> {
    crate::read_pinned(dir, MODEL_FILES[i], spec.files[i], spec.name, spec.repo)
}

/// The network: a BERT encoder, its pooler and the one-logit classifier
/// (`BertForSequenceClassification`).
struct Network {
    bert: BertModel,
    pooler: Linear,
    classifier: Linear,
    tokenizer: Tokenizer,
    device: Device,
}

type Failure = Box<dyn std::error::Error + Send + Sync>;

impl Network {
    fn load(dir: &Path, spec: &ModelSpec) -> Result<Self, LoadError> {
        let config = read_checked(dir, spec, 0)?;
        let tokenizer = read_checked(dir, spec, 1)?;
        let weights = read_checked(dir, spec, 2)?;
        let model = |e: &dyn fmt::Display| LoadError::Model(e.to_string());
        let config: Config = serde_json::from_slice(&config).map_err(|e| model(&e))?;
        // Truncation and padding are this adapter's own (`pair`), so the
        // tokenizer does neither.
        let mut tokenizer = Tokenizer::from_bytes(&tokenizer).map_err(|e| model(&e))?;
        tokenizer.with_truncation(None).map_err(|e| model(&e))?;
        tokenizer.with_padding(None);
        let device = Device::Cpu;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)
            .map_err(|e| model(&e))?;
        let bert = BertModel::load(vb.pp("bert"), &config).map_err(|e| model(&e))?;
        let pooler = linear(
            config.hidden_size,
            config.hidden_size,
            vb.pp("bert.pooler.dense"),
        )
        .map_err(|e| model(&e))?;
        let classifier =
            linear(config.hidden_size, 1, vb.pp("classifier")).map_err(|e| model(&e))?;
        Ok(Self {
            bert,
            pooler,
            classifier,
            tokenizer,
            device,
        })
    }

    /// The relevance of each passage to `query`, in order, each pair at
    /// most `max_tokens` long.
    fn score(
        &self,
        query: &str,
        passages: &[String],
        max_tokens: usize,
    ) -> Result<Vec<f32>, String> {
        self.score_pairs(query, passages, max_tokens.clamp(16, MAX_TOKENS))
            .map_err(|e| e.to_string())
    }

    fn score_pairs(
        &self,
        query: &str,
        passages: &[String],
        max_tokens: usize,
    ) -> Result<Vec<f32>, Failure> {
        if passages.is_empty() {
            return Ok(Vec::new());
        }
        // The query once, cut to its share; each passage cut in bytes before
        // the tokenizer reads it, then to what the query leaves (three
        // special tokens: [CLS] query [SEP] passage [SEP]).
        let mut q = self.tokenizer.encode_fast(query, false)?;
        q.truncate(max_tokens / QUERY_SHARE, 0, TruncationDirection::Right);
        let room = max_tokens.saturating_sub(q.len() + 3).max(1);
        let cut: Vec<&str> = passages.iter().map(|p| cut_passage(p)).collect();
        let mut pairs = Vec::with_capacity(cut.len());
        for mut p in self.tokenizer.encode_batch_fast(cut, false)? {
            p.truncate(room, 0, TruncationDirection::Right);
            pairs.push(self.tokenizer.post_process(q.clone(), Some(p), true)?);
        }
        // Batched by token count, so a short claim is not padded to an
        // abstract.
        let mut order: Vec<usize> = (0..pairs.len()).collect();
        order.sort_by_key(|&i| (pairs[i].len(), i));
        let mut out = vec![0.0; pairs.len()];
        for chunk in order.chunks(BATCH) {
            let batch: Vec<&Encoding> = chunk.iter().map(|&i| &pairs[i]).collect();
            for (&i, s) in chunk.iter().zip(self.batch(&batch)?) {
                out[i] = s;
            }
        }
        Ok(out)
    }

    fn batch(&self, encodings: &[&Encoding]) -> Result<Vec<f32>, Failure> {
        let rows = encodings.len();
        let width = encodings.iter().map(|e| e.len()).max().unwrap_or(0);
        // Padded to the batch's longest pair with id 0, type 0, mask 0.
        let tensor = |field: fn(&Encoding) -> &[u32]| -> Result<Tensor, Failure> {
            let mut flat = Vec::with_capacity(rows * width);
            for e in encodings {
                let values = field(e);
                flat.extend_from_slice(values);
                flat.resize(flat.len() + width - values.len(), 0);
            }
            Ok(Tensor::from_vec(flat, (rows, width), &self.device)?)
        };
        let ids = tensor(Encoding::get_ids)?;
        let types = tensor(Encoding::get_type_ids)?;
        let mask = tensor(Encoding::get_attention_mask)?;
        let hidden = self.bert.forward(&ids, &types, Some(&mask))?;
        let cls = hidden.i((.., 0))?;
        let pooled = self.pooler.forward(&cls)?.tanh()?;
        let logits = self.classifier.forward(&pooled)?.squeeze(D::Minus1)?;
        let relevance = candle_nn::ops::sigmoid(&logits)?;
        Ok(relevance.to_vec1::<f32>()?)
    }
}

/// A loaded cross-encoder, shared by every recall.
pub struct CrossEncoder {
    name: &'static str,
    network: Arc<Network>,
    max_tokens: usize,
    pool: Arc<rayon::ThreadPool>,
    slots: Arc<Semaphore>,
    timeout: Duration,
}

impl fmt::Debug for CrossEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CrossEncoder")
            .field("model", &self.name)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl CrossEncoder {
    /// Loads `spec`'s files from `dir`, each checked against its pinned
    /// digest first.
    ///
    /// # Errors
    ///
    /// [`LoadError::Missing`] or [`LoadError::Digest`] naming the file,
    /// [`LoadError::Model`] when the files do not make the network.
    pub fn load(dir: &Path, spec: &ModelSpec, limits: Limits) -> Result<Self, LoadError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(limits.threads.max(1))
            .thread_name(|i| format!("oml-rerank-{i}"))
            .build()
            .map_err(|e| LoadError::Model(e.to_string()))?;
        Ok(Self {
            name: spec.name,
            network: Arc::new(Network::load(dir, spec)?),
            max_tokens: limits.max_tokens,
            pool: Arc::new(pool),
            slots: Arc::new(Semaphore::new(limits.max_concurrent.max(1))),
            timeout: limits.timeout,
        })
    }

    /// Scores on the calling thread, with no cap and no deadline: for the
    /// eval and the tests, never the served path.
    ///
    /// # Errors
    ///
    /// The model's failure, named.
    pub fn score_now(&self, query: &str, passages: &[String]) -> Result<Vec<f32>, String> {
        self.pool
            .install(|| self.network.score(query, passages, self.max_tokens))
    }
}

impl Reranker for CrossEncoder {
    fn model(&self) -> &str {
        self.name
    }

    fn score(&self, query: String, passages: Vec<String>) -> RerankFuture<'_> {
        Box::pin(async move {
            if passages.is_empty() {
                return Ok(Vec::new());
            }
            let deadline = tokio::time::Instant::now() + self.timeout;
            let slot =
                match tokio::time::timeout_at(deadline, Arc::clone(&self.slots).acquire_owned())
                    .await
                {
                    Err(_) => return Err(RerankError::Busy),
                    Ok(Err(closed)) => return Err(RerankError::Failed(closed.to_string())),
                    Ok(Ok(slot)) => slot,
                };
            let network = Arc::clone(&self.network);
            let pool = Arc::clone(&self.pool);
            let max_tokens = self.max_tokens;
            let work = tokio::task::spawn_blocking(move || {
                let _slot = slot;
                pool.install(|| network.score(&query, &passages, max_tokens))
            });
            let failed = |reason: String| {
                // The error's own text: a tokenizer, tensor or join error,
                // which carries no passage text; what doctor's
                // `search_rerank_failed` line points at (ADR-0053 d9).
                tracing::warn!(event = "rerank_failed", model = self.name, %reason, "a rerank failed; the recall served the first stage's order");
                Err(RerankError::Failed(reason))
            };
            match tokio::time::timeout_at(deadline, work).await {
                Err(_) => Err(RerankError::Timeout),
                Ok(Err(join)) => failed(join.to_string()),
                Ok(Ok(Err(model))) => failed(model),
                Ok(Ok(Ok(scores))) => Ok(scores),
            }
        })
    }
}
