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

use std::path::Path;

use crate::LoadError;

/// The files a sparse model directory holds, in the order
/// [`SparseSpec::files`] pins them.
pub const SPARSE_FILES: [&str; 4] = [
    "config.json",
    "tokenizer.json",
    "model.safetensors",
    "idf.json",
];

/// A learned sparse model this build knows, pinned by revision and digest
/// (ADR-0052 d14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparseSpec {
    /// The name doctor and `/readyz` print, and the default directory's.
    pub name: &'static str,
    /// The Hugging Face repository the files are fetched from.
    pub repo: &'static str,
    /// The commit of that repository the digests were taken from.
    pub revision: &'static str,
    /// The licence its model card names.
    pub licence: &'static str,
    /// The SHA-256 of [`SPARSE_FILES`], lower-case hex.
    pub files: [&'static str; 4],
}

impl SparseSpec {
    /// The URL of `file` at the pinned revision.
    #[must_use]
    pub fn url(&self, file: &str) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{file}",
            self.repo, self.revision
        )
    }
}

/// `opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill`,
/// the model the offline experiment chose (ADR-0052 d14).
pub const DOC_V3_DISTILL: SparseSpec = SparseSpec {
    name: "opensearch-neural-sparse-encoding-doc-v3-distill",
    repo: "opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill",
    revision: "babf71f3c48695e2e53a978208e8aba48335e3c0",
    licence: "Apache-2.0",
    files: [
        "ee97780493e7d0a3b7b788ea98f3391e6be6b0b379921b465ca55bfdd0d9cbe3",
        "91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854",
        "83a3cc9757876b8590aac53f4f6685012f89d7fb4bbeb540815a54d325f7f70a",
        "da23a1c0b9252776cc8c6d70fd14723e218f484d489cd9027ac6e4065d5b9edd",
    ],
};

/// A document weight below this is not written: with
/// [`SPARSE_MAX_TOKENS`] it halves the postings for at most 0.003 of fused
/// nDCG@10 (ADR-0052 d14; the offline experiment's prune table).
pub const SPARSE_FLOOR: f32 = 0.1;

/// The most postings one version gets, its largest weights.
pub const SPARSE_MAX_TOKENS: usize = 128;

/// The tokens the model reads of a text, special tokens included: its
/// position embeddings' length, and the card's truncation.
pub const SPARSE_INPUT_TOKENS: usize = 512;

/// Fetches each of `spec`'s files missing from `dir` with `get`, checked
/// against the pinned digest and written under a `.partial` name first,
/// as the reranker's fetch does (ADR-0053 d3). Returns the files fetched.
///
/// # Errors
///
/// [`LoadError::Fetch`], [`LoadError::Digest`] or [`LoadError::Io`].
pub fn fetch_sparse_missing(
    dir: &Path,
    spec: &SparseSpec,
    get: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<&'static str>, LoadError> {
    crate::fetch_files(
        dir,
        spec.name,
        &SPARSE_FILES,
        &spec.files,
        |f| spec.url(f),
        get,
    )
}

/// Reads `dir/file` and checks it is the file `spec` pins.
fn read_checked(dir: &Path, spec: &SparseSpec, i: usize) -> Result<Vec<u8>, LoadError> {
    crate::read_pinned(
        dir,
        SPARSE_FILES[i],
        spec.files[i],
        spec.name,
        spec.repo,
        "search.sparse.fetch",
    )
}

/// Checks every pinned file in `dir` without parsing it: what a build
/// without the `model` feature can still say about a directory.
///
/// # Errors
///
/// [`LoadError::Missing`] or [`LoadError::Digest`] naming the file.
pub fn check_sparse_files(dir: &Path, spec: &SparseSpec) -> Result<(), LoadError> {
    for i in 0..SPARSE_FILES.len() {
        read_checked(dir, spec, i)?;
    }
    Ok(())
}

#[cfg(feature = "model")]
pub use model::SparseModel;

#[cfg(feature = "model")]
mod model {
    use std::collections::BTreeSet;
    use std::fmt;
    use std::path::Path;
    use std::sync::Arc;

    use candle_core::{DType, Device, Module, Tensor};
    use candle_nn::{Embedding, LayerNorm, Linear, VarBuilder, embedding, layer_norm, linear};
    use tokenizers::{Tokenizer, TruncationParams, TruncationStrategy};

    use oml_app::ports::{SparseEncoder, SparseFuture};
    use oml_app::search::{SparsePosting, SparseTerm};

    use super::{SPARSE_FLOOR, SPARSE_INPUT_TOKENS, SPARSE_MAX_TOKENS, SparseSpec, read_checked};
    use crate::LoadError;

    type Failure = Box<dyn std::error::Error + Send + Sync>;

    /// The shape `config.json` gives a DistilBERT.
    struct Config {
        vocab_size: usize,
        dim: usize,
        n_layers: usize,
        n_heads: usize,
        hidden_dim: usize,
        max_position_embeddings: usize,
    }

    impl Config {
        fn parse(bytes: &[u8]) -> Result<Self, String> {
            let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            let field = |k: &str| -> Result<usize, String> {
                v.get(k)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| format!("config.json has no {k}"))
            };
            Ok(Self {
                vocab_size: field("vocab_size")?,
                dim: field("dim")?,
                n_layers: field("n_layers")?,
                n_heads: field("n_heads")?,
                hidden_dim: field("hidden_dim")?,
                max_position_embeddings: field("max_position_embeddings")?,
            })
        }
    }

    struct Block {
        q: Linear,
        k: Linear,
        v: Linear,
        out: Linear,
        sa_norm: LayerNorm,
        lin1: Linear,
        lin2: Linear,
        out_norm: LayerNorm,
    }

    /// DistilBERT for masked language modelling, the exact GELU.
    struct Network {
        words: Embedding,
        positions: Embedding,
        embed_norm: LayerNorm,
        blocks: Vec<Block>,
        heads: usize,
        transform: Linear,
        vocab_norm: LayerNorm,
        projector: Linear,
    }

    impl Network {
        fn load(vb: &VarBuilder<'_>, c: &Config) -> candle_core::Result<Self> {
            let eps = 1e-12;
            let base = vb.pp("distilbert");
            let words = embedding(c.vocab_size, c.dim, base.pp("embeddings.word_embeddings"))?;
            let positions = embedding(
                c.max_position_embeddings,
                c.dim,
                base.pp("embeddings.position_embeddings"),
            )?;
            let embed_norm = layer_norm(c.dim, eps, base.pp("embeddings.LayerNorm"))?;
            let mut blocks = Vec::with_capacity(c.n_layers);
            for i in 0..c.n_layers {
                let l = base.pp(format!("transformer.layer.{i}"));
                blocks.push(Block {
                    q: linear(c.dim, c.dim, l.pp("attention.q_lin"))?,
                    k: linear(c.dim, c.dim, l.pp("attention.k_lin"))?,
                    v: linear(c.dim, c.dim, l.pp("attention.v_lin"))?,
                    out: linear(c.dim, c.dim, l.pp("attention.out_lin"))?,
                    sa_norm: layer_norm(c.dim, eps, l.pp("sa_layer_norm"))?,
                    lin1: linear(c.dim, c.hidden_dim, l.pp("ffn.lin1"))?,
                    lin2: linear(c.hidden_dim, c.dim, l.pp("ffn.lin2"))?,
                    out_norm: layer_norm(c.dim, eps, l.pp("output_layer_norm"))?,
                });
            }
            let transform = linear(c.dim, c.dim, vb.pp("vocab_transform"))?;
            let vocab_norm = layer_norm(c.dim, eps, vb.pp("vocab_layer_norm"))?;
            // The projector's weight is the word embeddings' (tied); its
            // bias is its own.
            let tied = base
                .pp("embeddings.word_embeddings")
                .get((c.vocab_size, c.dim), "weight")?;
            let bias = vb.pp("vocab_projector").get(c.vocab_size, "bias")?;
            Ok(Self {
                words,
                positions,
                embed_norm,
                blocks,
                heads: c.n_heads,
                transform,
                vocab_norm,
                projector: Linear::new(tied, Some(bias)),
            })
        }

        /// The masked-language logits of one unpadded sequence, `(len,
        /// vocab)`.
        fn logits(&self, ids: &[u32], device: &Device) -> candle_core::Result<Tensor> {
            let len = ids.len();
            let input = Tensor::new(ids, device)?.unsqueeze(0)?;
            let pos: Vec<u32> = (0..u32::try_from(len).unwrap_or(u32::MAX)).collect();
            let pos = Tensor::new(pos.as_slice(), device)?;
            let mut h = self
                .words
                .forward(&input)?
                .broadcast_add(&self.positions.forward(&pos)?)?;
            h = self.embed_norm.forward(&h)?;
            let (_, _, dim) = h.dims3()?;
            let head = dim / self.heads;
            #[allow(clippy::cast_precision_loss, reason = "a head is 64 wide")]
            let scale = (head as f64).sqrt();
            for b in &self.blocks {
                let split = |t: Tensor| -> candle_core::Result<Tensor> {
                    t.reshape((1, len, self.heads, head))?
                        .transpose(1, 2)?
                        .contiguous()
                };
                let q = (split(b.q.forward(&h)?)? / scale)?;
                let k = split(b.k.forward(&h)?)?;
                let v = split(b.v.forward(&h)?)?;
                let scores = q.matmul(&k.t()?.contiguous()?)?;
                let weights = candle_nn::ops::softmax_last_dim(&scores)?;
                let ctx = weights
                    .matmul(&v)?
                    .transpose(1, 2)?
                    .reshape((1, len, dim))?
                    .contiguous()?;
                let sa = b.sa_norm.forward(&(b.out.forward(&ctx)? + &h)?)?;
                let ff = b.lin2.forward(&b.lin1.forward(&sa)?.gelu_erf()?)?;
                h = b.out_norm.forward(&(ff + sa)?)?;
            }
            let t = self
                .vocab_norm
                .forward(&self.transform.forward(&h)?.gelu_erf()?)?;
            self.projector.forward(&t)?.squeeze(0)
        }
    }

    /// The loaded model: the tokenizer and idf table every query reads, and
    /// the network every document runs, on a pool of its own.
    #[derive(Clone)]
    pub struct SparseModel(Arc<Inner>);

    /// What a [`SparseModel`] shares between its handles.
    pub struct Inner {
        name: &'static str,
        tokenizer: Tokenizer,
        idf: Vec<f32>,
        special: BTreeSet<u32>,
        network: Arc<Network>,
        device: Device,
        pool: Arc<rayon::ThreadPool>,
    }

    impl fmt::Debug for SparseModel {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SparseModel")
                .field("model", &self.0.name)
                .finish_non_exhaustive()
        }
    }

    impl SparseModel {
        /// Loads `spec`'s files from `dir`, each checked against its pinned
        /// digest first, the network on a pool of `threads`.
        ///
        /// # Errors
        ///
        /// [`LoadError::Missing`] or [`LoadError::Digest`] naming the file,
        /// [`LoadError::Model`] when the files do not make the model.
        pub fn load(dir: &Path, spec: &SparseSpec, threads: usize) -> Result<Self, LoadError> {
            let model = |e: &dyn fmt::Display| LoadError::Model(e.to_string());
            let config = Config::parse(&read_checked(dir, spec, 0)?).map_err(LoadError::Model)?;
            let mut tokenizer =
                Tokenizer::from_bytes(read_checked(dir, spec, 1)?).map_err(|e| model(&e))?;
            tokenizer
                .with_truncation(Some(TruncationParams {
                    max_length: SPARSE_INPUT_TOKENS,
                    strategy: TruncationStrategy::LongestFirst,
                    ..TruncationParams::default()
                }))
                .map_err(|e| model(&e))?;
            tokenizer.with_padding(None);
            let weights = read_checked(dir, spec, 2)?;
            let idf_raw: std::collections::HashMap<String, f32> =
                serde_json::from_slice(&read_checked(dir, spec, 3)?).map_err(|e| model(&e))?;
            let mut idf = vec![0.0_f32; config.vocab_size];
            for (piece, w) in idf_raw {
                if let Some(slot) = tokenizer
                    .token_to_id(&piece)
                    .and_then(|id| idf.get_mut(id as usize))
                {
                    *slot = w;
                }
            }
            let special: BTreeSet<u32> = tokenizer
                .get_added_tokens_decoder()
                .into_iter()
                .filter(|(_, t)| t.special)
                .map(|(id, _)| id)
                .collect();
            let device = Device::Cpu;
            let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)
                .map_err(|e| model(&e))?;
            let network = Network::load(&vb, &config).map_err(|e| model(&e))?;
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads.max(1))
                .thread_name(|i| format!("oml-sparse-{i}"))
                .build()
                .map_err(|e| model(&e))?;
            Ok(Self(Arc::new(Inner {
                name: spec.name,
                tokenizer,
                idf,
                special,
                network: Arc::new(network),
                device,
                pool: Arc::new(pool),
            })))
        }
    }

    impl std::ops::Deref for SparseModel {
        type Target = Inner;
        fn deref(&self) -> &Inner {
            &self.0
        }
    }

    impl Inner {
        /// A text's input ids, special tokens included, truncated as the
        /// card truncates.
        ///
        /// # Errors
        ///
        /// The tokenizer's failure, named.
        pub fn input_ids(&self, text: &str) -> Result<Vec<u32>, String> {
            self.tokenizer
                .encode_fast(text, true)
                .map(|e| e.get_ids().to_vec())
                .map_err(|e| e.to_string())
        }

        /// Every non-zero weight of `text`, by token id, before the floor
        /// and the cap: the card's method, for the parity test.
        ///
        /// # Errors
        ///
        /// The tokenizer's or the network's failure, named.
        pub fn raw_weights(&self, text: &str) -> Result<Vec<(u32, f32)>, String> {
            self.pool
                .install(|| self.raw(text))
                .map_err(|e| e.to_string())
        }

        fn raw(&self, text: &str) -> Result<Vec<(u32, f32)>, Failure> {
            let ids = self.tokenizer.encode_fast(text, true)?.get_ids().to_vec();
            let logits = self.network.logits(&ids, &self.device)?;
            let v = logits
                .max(0)?
                .relu()?
                .affine(1.0, 1.0)?
                .log()?
                .affine(1.0, 1.0)?
                .log()?;
            let v: Vec<f32> = v.to_vec1()?;
            Ok(v.into_iter()
                .enumerate()
                .filter_map(|(i, w)| {
                    let id = u32::try_from(i).ok()?;
                    (w > 0.0 && !self.special.contains(&id)).then_some((id, w))
                })
                .collect())
        }

        /// One document's postings: the weights at or above the floor, the
        /// largest [`SPARSE_MAX_TOKENS`], each marked by whether the text's
        /// own tokens hold it (ADR-0052 d14, d17), sorted by token.
        ///
        /// # Errors
        ///
        /// The tokenizer's or the network's failure, named.
        pub fn document(&self, text: &str) -> Result<Vec<SparsePosting>, String> {
            self.pool
                .install(|| -> Result<Vec<SparsePosting>, Failure> {
                    let ids: BTreeSet<u32> = self
                        .tokenizer
                        .encode_fast(text, true)?
                        .get_ids()
                        .iter()
                        .copied()
                        .collect();
                    let mut kept: Vec<(u32, f32)> = self
                        .raw(text)?
                        .into_iter()
                        .filter(|(_, w)| *w >= SPARSE_FLOOR)
                        .collect();
                    kept.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                    kept.truncate(SPARSE_MAX_TOKENS);
                    kept.sort_by_key(|(id, _)| *id);
                    Ok(kept
                        .into_iter()
                        .map(|(token_id, weight)| SparsePosting {
                            token_id,
                            weight,
                            in_text: ids.contains(&token_id),
                        })
                        .collect())
                })
                .map_err(|e| e.to_string())
        }
    }

    impl SparseEncoder for SparseModel {
        fn model(&self) -> &str {
            self.name
        }

        fn encode_query(&self, query: &str) -> Vec<SparseTerm> {
            let Ok(encoding) = self.tokenizer.encode_fast(query, true) else {
                return Vec::new();
            };
            let mut ids: Vec<u32> = encoding
                .get_ids()
                .iter()
                .copied()
                .filter(|id| !self.special.contains(id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids.into_iter()
                .map(|token_id| SparseTerm {
                    token_id,
                    weight: self.idf.get(token_id as usize).copied().unwrap_or(0.0),
                    piece: self.tokenizer.id_to_token(token_id).unwrap_or_default(),
                })
                .collect()
        }

        fn encode_documents(&self, texts: Vec<String>) -> SparseFuture<'_> {
            Box::pin(async move {
                let this = self.clone();
                tokio::task::spawn_blocking(move || {
                    texts
                        .iter()
                        .map(|t| this.document(t))
                        .collect::<Result<Vec<_>, String>>()
                })
                .await
                .map_err(|e| e.to_string())?
            })
        }
    }
}
