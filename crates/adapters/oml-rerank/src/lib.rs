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

use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The network itself, behind the `model` feature: candle, tokenizers and
/// their numerics build only where it is on, so a crate that needs this
/// one's pins and settings alone (the lead built without `rerank`, the
/// CLI's test graph) never compiles them (ADR-0053 d2).
#[cfg(feature = "model")]
mod model;
#[cfg(feature = "model")]
pub use model::CrossEncoder;

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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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
