//! The local cross-encoder behind `oml-app`'s [`Reranker`] port (ADR-0053).
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
//! the deadline for a slot answers [`RerankError::Busy`], one whose model
//! runs past it [`RerankError::Timeout`], and the core then serves the first
//! stage's order (ADR-0053 d5). A late inference keeps its slot until it
//! ends, so the cap bounds the CPU actually in use.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, VarBuilder, linear};
use candle_transformers::models::bert::{BertModel, Config};
use sha2::{Digest, Sha256};
use tokenizers::{Encoding, Tokenizer, TruncationDirection};
use tokio::sync::Semaphore;

use oml_app::ports::{RerankError, RerankFuture, Reranker};

/// The files a model directory holds, in the order [`ModelSpec::files`]
/// pins them.
pub const MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];

/// A model this build knows: where its files come from and the SHA-256 of
/// each, so a file that is not the measured one is refused (ADR-0053 d3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// The name `search.rerank.model` takes, `/readyz` and doctor print.
    pub name: &'static str,
    /// The Hugging Face repository the files are fetched from.
    pub repo: &'static str,
    /// The commit of that repository the digests were taken from.
    pub revision: &'static str,
    /// The SHA-256 of `config.json`, `tokenizer.json` and
    /// `model.safetensors`, lower-case hex.
    pub files: [&'static str; 3],
}

impl ModelSpec {
    /// The URL of `file` at the pinned revision.
    #[must_use]
    pub fn url(&self, file: &str) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{file}",
            self.repo, self.revision
        )
    }
}

/// `cross-encoder/ms-marco-MiniLM-L6-v2`, Apache-2.0: six layers, the
/// default (ADR-0053 d2).
pub const MINILM_L6: ModelSpec = ModelSpec {
    name: "ms-marco-MiniLM-L6-v2",
    repo: "cross-encoder/ms-marco-MiniLM-L6-v2",
    revision: "233902d25c440f23af6f7d6e94d2946bac0bee0a",
    files: [
        "380e02c93f431831be65d99a4e7e5f67c133985bf2e77d9d4eba46847190bacc",
        "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
        "821d1aa69520101d6e0737f78a042ae25b19e5cb9160701909d10434f4aeb0ae",
    ],
};

/// `cross-encoder/ms-marco-MiniLM-L4-v2`, Apache-2.0: four layers, about a
/// third faster for a measured loss (ADR-0053 d2).
pub const MINILM_L4: ModelSpec = ModelSpec {
    name: "ms-marco-MiniLM-L4-v2",
    repo: "cross-encoder/ms-marco-MiniLM-L4-v2",
    revision: "777b2f369bc1c2f850df8bd367ed1654bda4497b",
    files: [
        "0349c3fb99c7c59d75529c12932e7663bf1d27ca4d8ee0a5202410466deba657",
        "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
        "3db1c98132b54901976d6da56fa711756c7780a6263d5db8f336232584f5fe58",
    ],
};

/// Every model this build serves.
pub const MODELS: [ModelSpec; 2] = [MINILM_L6, MINILM_L4];

/// The model a name spells, or `None` for one this build does not pin.
#[must_use]
pub fn model(name: &str) -> Option<ModelSpec> {
    MODELS.into_iter().find(|m| m.name == name)
}

/// The longest pair, in tokens, the model reads: the position embeddings'
/// length, and the ceiling of `search.rerank.max_tokens`.
pub const MAX_TOKENS: usize = 512;

/// The query's share of a pair is at most a quarter of its tokens, so a
/// pasted stack trace still leaves every passage three quarters of the pair
/// and the rerank answers rather than failing (ADR-0053 d2).
pub const QUERY_SHARE: usize = 4;

/// How many pairs run as one batch: a batch is padded to its longest pair,
/// so pairs are batched by token count. Sixteen holds the default head of
/// ten in one forward pass (ADR-0053 d4).
pub const BATCH: usize = 16;

/// Why a model could not be loaded or fetched; the lead refuses to start
/// naming it (ADR-0053 d8).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    #[error(
        "{dir}/{file} is missing: place the {model} files there, or set search.rerank.fetch = true to fetch them from {repo} at start"
    )]
    Missing {
        dir: String,
        file: &'static str,
        model: &'static str,
        repo: &'static str,
    },
    #[error(
        "{dir}/{file} has SHA-256 {found}, not the {expected} this build pins for {model}: remove it and fetch again"
    )]
    Digest {
        dir: String,
        file: &'static str,
        model: &'static str,
        expected: &'static str,
        found: String,
    },
    #[error("{0}")]
    Io(String),
    #[error("fetching {url}: {reason}")]
    Fetch { url: String, reason: String },
    #[error("the model would not load: {0}")]
    Model(String),
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Reads `dir/file` and checks it is the file `spec` pins.
fn read_checked(dir: &Path, spec: &ModelSpec, i: usize) -> Result<Vec<u8>, LoadError> {
    let file = MODEL_FILES[i];
    let path = dir.join(file);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(LoadError::Missing {
                dir: dir.display().to_string(),
                file,
                model: spec.name,
                repo: spec.repo,
            });
        }
        Err(e) => return Err(LoadError::Io(format!("{}: {e}", path.display()))),
    };
    let found = sha256_hex(&bytes);
    if found != spec.files[i] {
        return Err(LoadError::Digest {
            dir: dir.display().to_string(),
            file,
            model: spec.name,
            expected: spec.files[i],
            found,
        });
    }
    Ok(bytes)
}

/// Fetches each of `spec`'s files missing from `dir` with `get`, which
/// answers a URL's bytes, checks it against the pinned digest, and writes it
/// under a `.partial` name first, so a fetch cut short leaves no file a
/// later start would take for the model. A file already present is left to
/// [`CrossEncoder::load`] to check. Returns the files fetched.
///
/// # Errors
///
/// [`LoadError::Fetch`] when `get` fails, [`LoadError::Digest`] when a
/// fetched file is not the pinned one (nothing is written), and
/// [`LoadError::Io`] when the directory cannot be written.
pub fn fetch_missing(
    dir: &Path,
    spec: &ModelSpec,
    get: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<&'static str>, LoadError> {
    std::fs::create_dir_all(dir).map_err(|e| LoadError::Io(format!("{}: {e}", dir.display())))?;
    let mut fetched = Vec::new();
    for (i, file) in MODEL_FILES.into_iter().enumerate() {
        let path = dir.join(file);
        if path.exists() {
            continue;
        }
        let url = spec.url(file);
        let bytes = get(&url).map_err(|reason| LoadError::Fetch {
            url: url.clone(),
            reason,
        })?;
        let found = sha256_hex(&bytes);
        if found != spec.files[i] {
            return Err(LoadError::Digest {
                dir: dir.display().to_string(),
                file,
                model: spec.name,
                expected: spec.files[i],
                found,
            });
        }
        let partial = dir.join(format!("{file}.partial"));
        std::fs::write(&partial, &bytes)
            .and_then(|()| std::fs::rename(&partial, &path))
            .map_err(|e| LoadError::Io(format!("{}: {e}", path.display())))?;
        fetched.push(file);
    }
    Ok(fetched)
}

/// The cap, the threads and the deadline a [`CrossEncoder`] serves under
/// (configuration.md `search.rerank.*`; ADR-0053 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Inferences running at once; a call waits for a slot.
    pub max_concurrent: usize,
    /// The threads of the reranker's own pool, which every inference
    /// shares: the most CPU the stage takes. candle splits each product
    /// into its own count (`RAYON_NUM_THREADS`), which the lead sets to this
    /// before any thread starts, so the two agree (ADR-0053 d5).
    pub threads: usize,
    /// The longest pair in tokens, at most [`MAX_TOKENS`]: a passage is cut
    /// to what the query leaves of it (ADR-0053 d4).
    pub max_tokens: usize,
    /// From the call to its answer, waiting for a slot included.
    pub timeout: Duration,
}

/// The bytes of a passage the tokenizer reads: well past what 512 tokens
/// take in any script, so the cut never shortens what the model sees, and a
/// megabyte memory costs what its first 4 KiB cost (ADR-0053 d2).
pub const MAX_PASSAGE_BYTES: usize = 4_096;

/// `text` cut to [`MAX_PASSAGE_BYTES`] on a character boundary.
#[must_use]
pub fn cut_passage(text: &str) -> &str {
    if text.len() <= MAX_PASSAGE_BYTES {
        return text;
    }
    let mut end = MAX_PASSAGE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
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
