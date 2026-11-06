//! The lead composition pieces (workspace-plan.md: binaries are thin
//! compositions): configuration (oml.toml + `OML_` environment,
//! configuration.md), the role-split boot (ADR-0034 d4, d5), the kms-file
//! keyring (ADR-0023 d4), the runtime clock/id/scanner adapters, the
//! client document fetcher under its caps (ADR-0031 d5), the
//! startup fork check (ADR-0002 d7), the
//! startup naming and names check (ADR-0026 d11), and the start and state
//! constructors the integration tests share with `main`.

pub mod otlp;
pub mod timed;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::Row;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use axum::Router;
use oml_app::App;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::Provider;
use oml_app::domain::{EventTime, KeySet};
use oml_app::ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use oml_app::oauth_fetch::CappedFetcher;
use oml_app::ports::{
    AdmissionEnvelope, Clock, DocumentFetcher, IdGen, KeyringError, KeyringStore, Retriever as _,
    ScanOutcome, Scanner, Signer,
};
use oml_app::search::{Pipeline, SearchConfig};
use oml_app::types::AssignedNames;
use oml_fetch::PinnedFetcher;
use oml_fetch::signin::{ClientCredentials, ProviderClients};
use oml_pg::telemetry::PgTelemetry;
use oml_pg::{
    ForkCheckError, PgStore, RoleSplitCheckError, RoleSplitError, RoleSplitReason,
    role_split_postflight, role_split_preflight,
};
use oml_transport_http::traffic::{ScopeObserver, Traffic, TrustedProxies};
use oml_transport_http::web::WebPages;
use oml_transport_http::{ApiState, BuildInfo, DbPing, time::system_event_time};

use crate::timed::Timed;

// ---------------------------------------------------------------------------
// Configuration: defaults < file < environment (configuration.md)
// ---------------------------------------------------------------------------

/// The boot-configuration slice this composition reads (configuration.md
/// per-binary table: `postgres.*`, `kms.*`, `server.listen`, `oauth.issuer`;
/// `server.role` is not accepted in standalone — a standalone node is always
/// the lead).
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The serving role's URL, which the lead serves under (ADR-0034 d2, d4).
    pub postgres_url: String,
    /// The owner's URL, which the lead migrates under and opens again only
    /// for a new tenant's partition DDL (ADR-0034 d2, d4, d11).
    pub postgres_owner_url: String,
    pub listen: String,
    pub keyring_path: PathBuf,
    /// `oauth.issuer`, the public origin the connection flow is built from;
    /// unset, the OAuth routes do not exist and no connect code is minted
    /// (ADR-0031 d1).
    pub oauth_issuer: Option<Issuer>,
    /// `server.trusted_proxies`: the peers whose `X-Forwarded-For` the
    /// request line reads the client address from; empty, none
    /// (ADR-0042 d7).
    pub trusted_proxies: TrustedProxies,
    /// `server.metrics_listen`: the address of the listener that serves
    /// `GET /metrics` alone; unset, none binds (ADR-0042 d11).
    pub metrics_listen: Option<std::net::SocketAddr>,
    /// `server.otlp_endpoint`: the collector spans are exported to; unset,
    /// none are (ADR-0042 d13).
    pub otlp_endpoint: Option<otlp::Endpoint>,
    /// `web.google.client_id` and `web.google.client_secret`: Google
    /// sign-in, on when both are set (ADR-0045 d17).
    pub web_google: Option<ClientCredentials>,
    /// `web.github.client_id` and `web.github.client_secret`: GitHub
    /// sign-in, on when both are set (ADR-0045 d17).
    pub web_github: Option<ClientCredentials>,
    /// `brief.max_tokens`: the budget a workarea's brief is cut to, in
    /// budget tokens; 400 when unset, unmeasured (ADR-0046 d8; #145).
    pub brief_max_tokens: u64,
    /// `search.enabled` and `search.default`: the pipelines served and the
    /// default (ADR-0049 d4), with `search.rerank.depth` and
    /// `search.rerank.floor` (ADR-0053 d4, d6).
    pub search: SearchConfig,
    /// `search.rerank.*`: the model a reranking pipeline loads and how it
    /// is served (ADR-0053 d3, d5).
    pub rerank: RerankSettings,
    /// `search.sparse.*`: where the learned sparse model's files are,
    /// whether the lead may fetch them, and its pool (ADR-0052 d14).
    pub sparse: SparseSettings,
}

/// `search.sparse.*` (configuration.md; ADR-0052 d14): the model directory,
/// the fetch, and the encoder's own threads. A deployment serving no sparse
/// pipeline reads none of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparseSettings {
    pub model_dir: Option<PathBuf>,
    pub fetch: bool,
    pub threads: usize,
}

/// `search.sparse.threads`'s default: two of an 8-vCPU host's cores, as
/// the reranker's (ADR-0052 d14; ADR-0053 d5).
pub const DEFAULT_SPARSE_THREADS: usize = 2;

/// How many versions of one tenant the encoder takes per pass (ADR-0052
/// d15).
pub const SPARSE_PASS_PER_TENANT: usize = 16;

/// The shortest wait after a pass that found nothing; each idle pass in a
/// row doubles it up to [`SPARSE_IDLE_MAX_SECONDS`], and a pass with work
/// resets it, so an idle encoder costs one read every half minute
/// (ADR-0052 d15; MEDIUM-3 on review).
pub const SPARSE_IDLE_SECONDS: u64 = 1;

/// The longest wait between idle passes.
pub const SPARSE_IDLE_MAX_SECONDS: u64 = 30;

/// How often the backlog gauge is read while the passes find nothing; a
/// pass with work reads it after itself.
pub const SPARSE_GAUGE_SECONDS: u64 = 30;

impl Default for SparseSettings {
    fn default() -> Self {
        Self {
            model_dir: None,
            fetch: false,
            threads: DEFAULT_SPARSE_THREADS,
        }
    }
}

/// Loads the learned sparse encoder a configuration's sparse pipelines
/// read, or `None` when it serves none, which then reads no file and
/// fetches nothing: the missing files fetched first when
/// `search.sparse.fetch` allows it, then every file checked against its
/// pinned digest (ADR-0052 d14, d16). Blocking: it reads the model's files.
///
/// # Errors
///
/// The model's file or fetch refusal, naming the file and the remedy; a
/// sparse pipeline never falls back to another (ADR-0052 d16).
pub fn load_sparse(
    search: &SearchConfig,
    settings: &SparseSettings,
    get: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Option<Arc<dyn oml_app::ports::SparseEncoder>>, String> {
    if !search.needs_sparse() {
        return Ok(None);
    }
    let spec = oml_rerank::DOC_V3_DISTILL;
    let model_dir = settings.model_dir.as_deref().ok_or_else(|| {
        "search.sparse.model_dir is unset and no state directory is known (STATE_DIRECTORY, XDG_STATE_HOME or HOME): set an absolute search.sparse.model_dir".to_owned()
    })?;
    if settings.fetch {
        let fetched =
            oml_rerank::fetch_sparse_missing(model_dir, &spec, get).map_err(|e| e.to_string())?;
        if !fetched.is_empty() {
            tracing::info!(
                event = "sparse_model_fetched",
                model = spec.name,
                files = fetched.join(","),
                "fetched the sparse encoder's files at its pinned revision"
            );
        }
    }
    #[cfg(feature = "rerank")]
    {
        let model = oml_rerank::SparseModel::load(model_dir, &spec, settings.threads)
            .map_err(|e| e.to_string())?;
        Ok(Some(Arc::new(model)))
    }
    #[cfg(not(feature = "rerank"))]
    {
        oml_rerank::check_sparse_files(model_dir, &spec).map_err(|e| e.to_string())?;
        Err("search.enabled names a sparse pipeline and this lead was built without its local models (the rerank feature)".to_owned())
    }
}

/// The encoder task (ADR-0052 d15): a pass over every tenant's due
/// versions, at once again while a pass finds work, else after
/// [`SPARSE_IDLE_SECONDS`]; each pass's counts to the metrics, and the
/// backlog after it. A failed pass is logged and never stops the lead or
/// the next pass.
pub fn spawn_sparse_encoder(
    store: PgStore,
    encoder: Arc<dyn oml_app::ports::SparseEncoder>,
    metrics: Arc<oml_transport_http::traffic::Metrics>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = SPARSE_IDLE_SECONDS;
        let mut gauged: Option<std::time::Instant> = None;
        loop {
            let busy = match oml_app::app::encode_sparse_pass(
                &store,
                encoder.as_ref(),
                SPARSE_PASS_PER_TENANT,
            )
            .await
            {
                Ok(pass) => {
                    metrics.sparse_pass(pass.written, pass.failed, pass.parked);
                    if pass.failed > 0 {
                        tracing::warn!(
                            event = "sparse_encode_failed",
                            failed = pass.failed,
                            parked = pass.parked,
                            "versions failed to encode; they stay queued with their reason, and one failing too often is parked (search_sparse_queue.last_error, parked_at)"
                        );
                    }
                    pass.busy()
                }
                Err(e) => {
                    tracing::error!(event = "sparse_pass_failed", reason = %e, "the sparse encoder's pass failed; the next pass retries");
                    false
                }
            };
            // After every pass with work, so the gauge falls to zero as the
            // queue drains, and every half minute while idle.
            if busy || gauged.is_none_or(|t| t.elapsed().as_secs() >= SPARSE_GAUGE_SECONDS) {
                if let Ok(waiting) = store.sparse_backlog().await {
                    metrics.sparse_backlog(waiting);
                }
                gauged = Some(std::time::Instant::now());
            }
            if busy {
                idle = SPARSE_IDLE_SECONDS;
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(idle)).await;
                idle = (idle * 2).min(SPARSE_IDLE_MAX_SECONDS);
            }
        }
    })
}

/// `search.rerank.*` beyond the policy the core reads (configuration.md;
/// ADR-0053 d3, d5): which pinned model, where its files are, whether the
/// lead may fetch the missing ones at start, and the cap and deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankSettings {
    pub model: oml_rerank::ModelSpec,
    pub model_dir: Option<PathBuf>,
    pub fetch: bool,
    pub limits: oml_rerank::Limits,
}

/// `search.rerank.max_concurrent`'s default (ADR-0053 d5).
pub const DEFAULT_RERANK_MAX_CONCURRENT: usize = 2;

/// `search.rerank.threads`'s default: two of an 8-vCPU host's cores, the
/// rest left to tokio's workers and PostgreSQL (ADR-0053 d5).
pub const DEFAULT_RERANK_THREADS: usize = 2;

/// `search.rerank.max_tokens`'s default: 256, half the model's window.
/// Measured: the recall eval's claims rank the same at 256 as at 512, and
/// SciFact's abstracts rerank 2.6 times faster for 0.001 nDCG@10
/// (ADR-0053 d4).
pub const DEFAULT_RERANK_MAX_TOKENS: usize = 256;

/// The shortest `search.rerank.max_tokens`: room for a query's share and a
/// passage.
const MIN_RERANK_MAX_TOKENS: u64 = 64;

/// The model directory under the process's state directory: systemd's
/// `STATE_DIRECTORY`, else `$XDG_STATE_HOME/oml`, else
/// `$HOME/.local/state/oml`, each only when absolute; never the working
/// directory (ADR-0053 d3).
fn state_model_dir(env: &dyn Fn(&str) -> Option<String>, model: &str) -> Option<PathBuf> {
    let absolute = |v: String| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    let state = env("STATE_DIRECTORY")
        .and_then(|v| v.split(':').next().map(ToOwned::to_owned))
        .and_then(absolute)
        .or_else(|| {
            env("XDG_STATE_HOME")
                .and_then(absolute)
                .map(|p| p.join("oml"))
        })
        .or_else(|| {
            env("HOME")
                .and_then(absolute)
                .map(|p| p.join(".local/state/oml"))
        })?;
    Some(state.join("models").join(model))
}

/// `search.rerank.timeout_ms`'s default (ADR-0053 d5).
pub const DEFAULT_RERANK_TIMEOUT_MS: u64 = 1_000;

impl Default for RerankSettings {
    fn default() -> Self {
        let model = oml_rerank::MINILM_L6;
        Self {
            model,
            model_dir: None,
            fetch: false,
            limits: oml_rerank::Limits {
                max_concurrent: DEFAULT_RERANK_MAX_CONCURRENT,
                threads: DEFAULT_RERANK_THREADS,
                max_tokens: DEFAULT_RERANK_MAX_TOKENS,
                timeout: std::time::Duration::from_millis(DEFAULT_RERANK_TIMEOUT_MS),
            },
        }
    }
}

/// The largest model file the lead fetches: the pinned weights are about
/// 91 MB, so anything far larger is not the pinned file.
const MODEL_FILE_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Fetches one pinned model file over HTTPS, following the repository's
/// redirect to its storage; the caller checks the digest (ADR-0053 d3).
///
/// # Errors
///
/// The transport's failure or a status other than 200, named.
pub fn fetch_model_file(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MODEL_FILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MODEL_FILE_MAX_BYTES {
        return Err(format!(
            "the answer is larger than {MODEL_FILE_MAX_BYTES} bytes, so not the pinned file; nothing was written"
        ));
    }
    Ok(bytes)
}

/// Loads the reranker a configuration's reranking pipelines call, or
/// `None` when it serves none: the missing files fetched first when
/// `search.rerank.fetch` allows it, through `get`, then every file checked
/// against its pinned digest (ADR-0053 d3, d8). Blocking: it reads the
/// model's files.
///
/// # Errors
///
/// The model's file or fetch refusal, naming the file and the remedy.
pub fn load_reranker(
    search: &SearchConfig,
    settings: &RerankSettings,
    get: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Option<Arc<dyn oml_app::ports::Reranker>>, String> {
    if !search.needs_reranker() {
        return Ok(None);
    }
    let model_dir = settings.model_dir.as_deref().ok_or_else(|| {
        "search.rerank.model_dir is unset and no state directory is known (STATE_DIRECTORY, XDG_STATE_HOME or HOME): set an absolute search.rerank.model_dir".to_owned()
    })?;
    if settings.fetch {
        let fetched = oml_rerank::fetch_missing(model_dir, &settings.model, get)
            .map_err(|e| e.to_string())?;
        if !fetched.is_empty() {
            tracing::info!(
                event = "rerank_model_fetched",
                model = settings.model.name,
                files = fetched.join(","),
                "fetched the reranker's files at its pinned revision"
            );
        }
    }
    #[cfg(feature = "rerank")]
    {
        let model = oml_rerank::CrossEncoder::load(model_dir, &settings.model, settings.limits)
            .map_err(|e| e.to_string())?;
        Ok(Some(Arc::new(model)))
    }
    #[cfg(not(feature = "rerank"))]
    {
        let _ = model_dir;
        Err("search.enabled names a reranking pipeline and this lead was built without its rerank feature".to_owned())
    }
}

/// `search.enabled`'s default: the pipelines that need no extension.
/// `keyword-all`, which the recall eval runs for comparison, and
/// `bm25-trigram`, which needs `pg_trgm`, are served only where a
/// deployment enables them, so a database without the extension starts
/// (configuration.md search.enabled; ADR-0049 d4, d12). `bm25` stays
/// served beside `bm25-postings`: it is the postings' fallback statement
/// and their comparison (ADR-0052 d9).
pub const DEFAULT_SEARCH_ENABLED: [Pipeline; 3] =
    [Pipeline::KeywordAny, Pipeline::Bm25, Pipeline::Bm25Postings];

/// `search.default`'s default: `bm25-postings`, moved from `bm25` by
/// ADR-0052 d9's evidence, measured on the build that ships it (66979b4,
/// 2026-11-05): `bm25`'s recall-eval gate passes with `bm25`'s own
/// success@1 0.826 and success@5 0.934; nDCG@10, R@20 and R@100 equal
/// `bm25`'s on SciFact, NFCorpus and LoCoMo; and the p95 at 99,721 readable
/// rows, universe scope, is 25.5 ms against `bm25`'s 485 ms
/// (bench/scale/REPORT.md; eval/bench/REPORT.md).
pub const DEFAULT_SEARCH_PIPELINE: Pipeline = Pipeline::Bm25Postings;

/// The search settings of a deployment that sets none.
#[must_use]
pub fn default_search() -> SearchConfig {
    SearchConfig::new(&DEFAULT_SEARCH_ENABLED, DEFAULT_SEARCH_PIPELINE)
        .expect("the default pipeline is among the default enabled set")
}

/// Why a configuration is refused: `postgres.owner_url` unset is the role
/// split's `owner_url_missing`, named as the boot's other refusals are
/// (ADR-0034 d5); anything else is a configuration that does not load.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error(transparent)]
    RoleSplit(#[from] RoleSplitError),
    #[error("{0}")]
    Invalid(String),
}

const KEY_POSTGRES_URL: &str = "postgres.url";
const KEY_POSTGRES_OWNER_URL: &str = "postgres.owner_url";
const KEY_LISTEN: &str = "server.listen";
const KEY_KMS_ADAPTER: &str = "kms.adapter";
const KEY_KEYRING_PATH: &str = "kms.keyring_path";
const KEY_OAUTH_ISSUER: &str = "oauth.issuer";
const KEY_TRUSTED_PROXIES: &str = "server.trusted_proxies";
const KEY_METRICS_LISTEN: &str = "server.metrics_listen";
const KEY_OTLP_ENDPOINT: &str = "server.otlp_endpoint";
const KEY_WEB_GOOGLE_CLIENT_ID: &str = "web.google.client_id";
const KEY_WEB_GOOGLE_CLIENT_SECRET: &str = "web.google.client_secret";
const KEY_WEB_GITHUB_CLIENT_ID: &str = "web.github.client_id";
const KEY_WEB_GITHUB_CLIENT_SECRET: &str = "web.github.client_secret";
const KEY_BRIEF_MAX_TOKENS: &str = "brief.max_tokens";
const KEY_SEARCH_ENABLED: &str = "search.enabled";
const KEY_SEARCH_DEFAULT: &str = "search.default";
const KEY_RERANK_MODEL: &str = "search.rerank.model";
const KEY_RERANK_MODEL_DIR: &str = "search.rerank.model_dir";
const KEY_RERANK_FETCH: &str = "search.rerank.fetch";
/// Whether a recall may ask for its per-stage timings (ADR-0049 d14).
const KEY_SEARCH_TIMINGS: &str = "search.timings";
const KEY_RERANK_DEPTH: &str = "search.rerank.depth";
const KEY_RERANK_FLOOR: &str = "search.rerank.floor";
const KEY_RERANK_MAX_CONCURRENT: &str = "search.rerank.max_concurrent";
const KEY_RERANK_TIMEOUT_MS: &str = "search.rerank.timeout_ms";
const KEY_RERANK_THREADS: &str = "search.rerank.threads";
const KEY_RERANK_MAX_TOKENS: &str = "search.rerank.max_tokens";
const KEY_SPARSE_MODEL_DIR: &str = "search.sparse.model_dir";
const KEY_SPARSE_FETCH: &str = "search.sparse.fetch";
const KEY_SPARSE_THREADS: &str = "search.sparse.threads";

/// `OML_` + dotted path in SCREAMING_SNAKE with `__` as the nesting
/// separator — mechanical in both directions (configuration.md).
#[must_use]
pub fn env_name(key: &str) -> String {
    format!("OML_{}", key.to_uppercase().replace('.', "__"))
}

fn file_lookup<'a>(doc: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut node = doc;
    for part in key.split('.') {
        node = node.get(part)?;
    }
    Some(node)
}

impl Config {
    /// Composes defaults, the parsed TOML document and the environment, in
    /// that precedence (configuration.md: defaults < file < environment).
    /// `env` is injected so the precedence law is testable without mutating
    /// the process environment.
    /// `postgres.owner_url` is required, and its absence is refused as
    /// `postgres.role_split` `owner_url_missing`: no single-URL mode exists
    /// (ADR-0034 d4, d5).
    pub fn from_sources(
        file: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let doc: Option<toml::Value> = match file {
            None => None,
            Some(text) => Some(text.parse::<toml::Value>().map_err(|e| {
                ConfigError::Invalid(format!("the configuration file is not TOML: {e}"))
            })?),
        };
        let lookup = |key: &str| -> Option<String> {
            if let Some(v) = env(&env_name(key)) {
                return Some(v);
            }
            doc.as_ref()
                .and_then(|d| file_lookup(d, key))
                .and_then(|v| {
                    v.as_str()
                        .map(ToOwned::to_owned)
                        .or_else(|| v.as_integer().map(|n| n.to_string()))
                        .or_else(|| v.as_float().map(|n| n.to_string()))
                        .or_else(|| v.as_bool().map(|b| b.to_string()))
                })
        };
        // The PoC composes the kms-file adapter only (ADR-0023 d4); naming
        // another adapter is refused rather than silently ignored.
        if let Some(adapter) = lookup(KEY_KMS_ADAPTER)
            && adapter != "file"
        {
            return Err(ConfigError::Invalid(format!(
                "kms.adapter {adapter:?} is not available in this composition (kms-file only, ADR-0023 d4)"
            )));
        }
        let postgres_url = lookup(KEY_POSTGRES_URL).ok_or_else(|| {
            ConfigError::Invalid(format!(
                "{KEY_POSTGRES_URL} is required (none — required, configuration.md)"
            ))
        })?;
        let postgres_owner_url = lookup(KEY_POSTGRES_OWNER_URL).ok_or(RoleSplitError {
            reason: RoleSplitReason::OwnerUrlMissing,
        })?;
        let listen = lookup(KEY_LISTEN).unwrap_or_else(|| "127.0.0.1:8080".to_owned());
        let keyring_path =
            lookup(KEY_KEYRING_PATH).map_or_else(|| PathBuf::from("oml-keyring"), PathBuf::from);
        // A value that is not exactly a public origin stops the start
        // (configuration.md, oauth.issuer; ADR-0031 d1).
        let oauth_issuer = lookup(KEY_OAUTH_ISSUER)
            .map(|value| Issuer::parse(&value))
            .transpose()
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        // A TOML array in the file, comma-separated in the environment; an
        // entry that does not parse stops the start (configuration.md,
        // server.trusted_proxies; ADR-0042 d7).
        let proxies: Vec<String> = match env(&env_name(KEY_TRUSTED_PROXIES)) {
            Some(list) => list
                .split(',')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            None => match doc
                .as_ref()
                .and_then(|d| file_lookup(d, KEY_TRUSTED_PROXIES))
            {
                None => Vec::new(),
                Some(toml::Value::Array(entries)) => entries
                    .iter()
                    .map(|e| {
                        e.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                            ConfigError::Invalid(format!(
                                "{KEY_TRUSTED_PROXIES} entries are strings (configuration.md)"
                            ))
                        })
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => {
                    return Err(ConfigError::Invalid(format!(
                        "{KEY_TRUSTED_PROXIES} is an array of addresses and CIDR ranges (configuration.md)"
                    )));
                }
            },
        };
        let trusted_proxies = TrustedProxies::parse(&proxies).map_err(ConfigError::Invalid)?;
        // A second listener, never the API's: a value equal to
        // server.listen, or one that is not an address, stops the start
        // (configuration.md, server.metrics_listen; ADR-0042 d11).
        let metrics_listen = match lookup(KEY_METRICS_LISTEN) {
            None => None,
            Some(value) => {
                let addr: std::net::SocketAddr = value.parse().map_err(|_| {
                    ConfigError::Invalid(format!(
                        "{KEY_METRICS_LISTEN} {value:?} is not an address and port (configuration.md)"
                    ))
                })?;
                if value == listen || listen.parse::<std::net::SocketAddr>() == Ok(addr) {
                    return Err(ConfigError::Invalid(format!(
                        "{KEY_METRICS_LISTEN} equals {KEY_LISTEN}: /metrics is never served on the API listener (ADR-0042 d11)"
                    )));
                }
                Some(addr)
            }
        };
        // Only OML's setting turns export on; OTEL_* is never read
        // (configuration.md, server.otlp_endpoint; ADR-0042 d13).
        let otlp_endpoint = lookup(KEY_OTLP_ENDPOINT)
            .map(|value| otlp::Endpoint::parse(&value))
            .transpose()
            .map_err(ConfigError::Invalid)?;
        // A provider is on when both of its values are set; one without the
        // other, or a provider without oauth.issuer, whose origin is the
        // callback's, stops the start (configuration.md web.*; ADR-0045 d17).
        let provider = |id_key: &str,
                        secret_key: &str|
         -> Result<Option<ClientCredentials>, ConfigError> {
            match (lookup(id_key), lookup(secret_key)) {
                (None, None) => Ok(None),
                (Some(client_id), Some(client_secret))
                    if !client_id.is_empty() && !client_secret.is_empty() =>
                {
                    if oauth_issuer.is_none() {
                        return Err(ConfigError::Invalid(format!(
                            "{id_key} is set and {KEY_OAUTH_ISSUER} is not: the sign-in callback is <oauth.issuer>/web/callback/<provider> (ADR-0045 d17)"
                        )));
                    }
                    Ok(Some(ClientCredentials {
                        client_id,
                        client_secret,
                    }))
                }
                _ => Err(ConfigError::Invalid(format!(
                    "{id_key} and {secret_key} are set together or not at all (ADR-0045 d17)"
                ))),
            }
        };
        let web_google = provider(KEY_WEB_GOOGLE_CLIENT_ID, KEY_WEB_GOOGLE_CLIENT_SECRET)?;
        let web_github = provider(KEY_WEB_GITHUB_CLIENT_ID, KEY_WEB_GITHUB_CLIENT_SECRET)?;
        // A brief that serves nothing is no brief: zero and anything but a
        // positive whole number stop the start (ADR-0046 d8).
        let brief_max_tokens = match lookup(KEY_BRIEF_MAX_TOKENS) {
            None => oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
            Some(value) => value.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "{KEY_BRIEF_MAX_TOKENS} {value:?} is not a positive whole number of budget tokens (configuration.md)"
                ))
            })?,
        };
        // A TOML array in the file, comma-separated in the environment; an
        // unknown name, an empty set or a default outside it stops the start
        // (configuration.md search.*; ADR-0049 d4).
        let enabled_names: Option<Vec<String>> = match env(&env_name(KEY_SEARCH_ENABLED)) {
            Some(list) => Some(
                list.split(',')
                    .map(str::trim)
                    .filter(|e| !e.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
            ),
            None => match doc.as_ref().and_then(|d| file_lookup(d, KEY_SEARCH_ENABLED)) {
                None => None,
                Some(toml::Value::Array(entries)) => Some(
                    entries
                        .iter()
                        .map(|e| {
                            e.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                                ConfigError::Invalid(format!(
                                    "{KEY_SEARCH_ENABLED} entries are pipeline names (configuration.md)"
                                ))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                ),
                Some(_) => {
                    return Err(ConfigError::Invalid(format!(
                        "{KEY_SEARCH_ENABLED} is an array of pipeline names (configuration.md)"
                    )));
                }
            },
        };
        let enabled = match enabled_names {
            None => DEFAULT_SEARCH_ENABLED.to_vec(),
            Some(names) => SearchConfig::parse_names(&names)
                .map_err(|e| ConfigError::Invalid(e.to_string()))?,
        };
        let default = match lookup(KEY_SEARCH_DEFAULT) {
            None => DEFAULT_SEARCH_PIPELINE,
            Some(name) => Pipeline::parse(name.trim()).ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "{KEY_SEARCH_DEFAULT} {name:?} is not a pipeline this build serves (configuration.md)"
                ))
            })?,
        };
        // The reranking stage: a model this build pins, its directory, and
        // the depth, floor, cap and deadline, each refused naming the key
        // when it does not parse (configuration.md search.rerank.*;
        // ADR-0053 d3-d6).
        let whole = |key: &str, min: u64| -> Result<Option<u64>, ConfigError> {
            lookup(key)
                .map(|v| {
                    v.trim().parse::<u64>().ok().filter(|n| *n >= min).ok_or_else(|| {
                        ConfigError::Invalid(format!(
                            "{key} {v:?} is not a whole number of at least {min} (configuration.md)"
                        ))
                    })
                })
                .transpose()
        };
        let model = match lookup(KEY_RERANK_MODEL) {
            None => oml_rerank::MINILM_L6,
            Some(name) => oml_rerank::model(name.trim()).ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "{KEY_RERANK_MODEL} {name:?} is not a model this build pins (known: {})",
                    oml_rerank::MODELS.map(|m| m.name).join(", ")
                ))
            })?,
        };
        let model_dir = match lookup(KEY_RERANK_MODEL_DIR) {
            None => state_model_dir(env, model.name),
            Some(dir) if PathBuf::from(&dir).is_absolute() => Some(PathBuf::from(dir)),
            Some(dir) => {
                return Err(ConfigError::Invalid(format!(
                    "{KEY_RERANK_MODEL_DIR} {dir:?} is not an absolute path: a relative one would follow the working directory (configuration.md)"
                )));
            }
        };
        let fetch = match lookup(KEY_RERANK_FETCH).as_deref().map(str::trim) {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(ConfigError::Invalid(format!(
                    "{KEY_RERANK_FETCH} {other:?} is not true or false (configuration.md)"
                )));
            }
        };
        let depth = whole(KEY_RERANK_DEPTH, 1)?.map_or(oml_app::search::RERANK_DEPTH, |n| {
            usize::try_from(n).unwrap_or(usize::MAX)
        });
        let floor = match lookup(KEY_RERANK_FLOOR).as_deref().map(str::trim) {
            None => oml_app::search::RERANK_FLOOR,
            Some("off") => None,
            Some(v) => Some(v.parse::<f32>().map_err(|_| {
                ConfigError::Invalid(format!(
                    "{KEY_RERANK_FLOOR} {v:?} is not a relevance between 0 and 1, or off (configuration.md)"
                ))
            })?),
        };
        let policy = oml_app::search::RerankPolicy::new(depth, floor)
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        let max_concurrent = whole(KEY_RERANK_MAX_CONCURRENT, 1)?
            .map_or(DEFAULT_RERANK_MAX_CONCURRENT, |n| {
                usize::try_from(n).unwrap_or(usize::MAX)
            });
        let timeout_ms = whole(KEY_RERANK_TIMEOUT_MS, 1)?.unwrap_or(DEFAULT_RERANK_TIMEOUT_MS);
        let threads = whole(KEY_RERANK_THREADS, 1)?.map_or(DEFAULT_RERANK_THREADS, |n| {
            usize::try_from(n).unwrap_or(usize::MAX)
        });
        let max_tokens = match whole(KEY_RERANK_MAX_TOKENS, MIN_RERANK_MAX_TOKENS)? {
            None => DEFAULT_RERANK_MAX_TOKENS,
            Some(n) if n <= oml_rerank::MAX_TOKENS as u64 => {
                usize::try_from(n).unwrap_or(oml_rerank::MAX_TOKENS)
            }
            Some(n) => {
                return Err(ConfigError::Invalid(format!(
                    "{KEY_RERANK_MAX_TOKENS} {n} is past the model's {} tokens (configuration.md)",
                    oml_rerank::MAX_TOKENS
                )));
            }
        };
        let rerank = RerankSettings {
            model,
            model_dir,
            fetch,
            limits: oml_rerank::Limits {
                max_concurrent,
                threads,
                max_tokens,
                timeout: std::time::Duration::from_millis(timeout_ms),
            },
        };
        // The learned sparse encoder: its directory, fetch and pool, each
        // refused naming the key when it does not parse (configuration.md
        // search.sparse.*; ADR-0052 d14).
        let sparse = SparseSettings {
            model_dir: match lookup(KEY_SPARSE_MODEL_DIR) {
                None => state_model_dir(env, oml_rerank::DOC_V3_DISTILL.name),
                Some(dir) if PathBuf::from(&dir).is_absolute() => Some(PathBuf::from(dir)),
                Some(dir) => {
                    return Err(ConfigError::Invalid(format!(
                        "{KEY_SPARSE_MODEL_DIR} {dir:?} is not an absolute path: a relative one would follow the working directory (configuration.md)"
                    )));
                }
            },
            fetch: match lookup(KEY_SPARSE_FETCH).as_deref().map(str::trim) {
                None | Some("false") => false,
                Some("true") => true,
                Some(other) => {
                    return Err(ConfigError::Invalid(format!(
                        "{KEY_SPARSE_FETCH} {other:?} is not true or false (configuration.md)"
                    )));
                }
            },
            threads: whole(KEY_SPARSE_THREADS, 1)?.map_or(DEFAULT_SPARSE_THREADS, |n| {
                usize::try_from(n).unwrap_or(usize::MAX)
            }),
        };
        let timings = match lookup(KEY_SEARCH_TIMINGS).as_deref().map(str::trim) {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(ConfigError::Invalid(format!(
                    "{KEY_SEARCH_TIMINGS} {other:?} is not true or false (configuration.md)"
                )));
            }
        };
        let search = SearchConfig::new(&enabled, default)
            .map_err(|e| ConfigError::Invalid(e.to_string()))?
            .with_rerank(policy)
            .with_timings(timings);
        Ok(Self {
            postgres_url,
            postgres_owner_url,
            listen,
            keyring_path,
            oauth_issuer,
            trusted_proxies,
            metrics_listen,
            otlp_endpoint,
            web_google,
            web_github,
            brief_max_tokens,
            search,
            rerank,
            sparse,
        })
    }

    /// The web pages over the configured providers, or `None` when no
    /// provider is set and the pages are off (ADR-0045 d1, d17).
    #[must_use]
    pub fn web_pages(&self) -> Option<WebPages> {
        let mut providers = Vec::new();
        if let Some(google) = &self.web_google {
            providers.push((Provider::Google, google.client_id.clone()));
        }
        if let Some(github) = &self.web_github {
            providers.push((Provider::GitHub, github.client_id.clone()));
        }
        (!providers.is_empty()).then(|| {
            WebPages::new(
                providers,
                ProviderClients {
                    google: self.web_google.clone(),
                    github: self.web_github.clone(),
                },
            )
        })
    }

    /// Reads `--config <path>` (when given) and the process environment.
    pub fn load(
        config_path: Option<&Path>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let file = match config_path {
            None => None,
            Some(path) => Some(std::fs::read_to_string(path).map_err(|e| {
                ConfigError::Invalid(format!("cannot read {}: {e}", path.display()))
            })?),
        };
        Self::from_sources(file.as_deref(), env)
    }
}

// ---------------------------------------------------------------------------
// The role-split boot (ADR-0034 d4, d5)
// ---------------------------------------------------------------------------

/// Why the role-split boot did not finish: a `postgres.role_split`
/// refusal, by reason or as a catalog read that failed (ADR-0034 d5), or a
/// connection or migration that failed.
#[derive(Debug, thiserror::Error)]
pub enum SplitBootError {
    #[error(transparent)]
    RoleSplit(#[from] RoleSplitCheckError),
    #[error("postgres owner connect: {0}")]
    OwnerConnect(sqlx::Error),
    #[error("migrate on boot: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("postgres login: {0}")]
    Login(sqlx::Error),
}

/// The two logins the boot migrated and serves under, as each URL's
/// `current_user` reads them (ADR-0034 d2: no role name is configured).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRoles {
    pub owner: String,
    pub serving: String,
}

/// The lead's boot before its keyring (ADR-0034 d4): over `store`'s
/// serving pool and an owner pool opened at `owner_url` with
/// `oml.serving_role` set, d5's checks before migrating, the migrations on
/// the owner pool, d5's checks after, then the owner pool closed on every
/// path, a refusal's included, so no owner connection outlives the boot
/// but d11's partition DDL (d8). The caller binds no listener unless this
/// returns `Ok`.
pub async fn split_boot(store: &PgStore, owner_url: &str) -> Result<SplitRoles, SplitBootError> {
    let owner = store
        .connect_owner(owner_url)
        .await
        .map_err(SplitBootError::OwnerConnect)?;
    let booted = split_boot_on(store, &owner).await;
    owner.close().await;
    booted
}

async fn split_boot_on(store: &PgStore, owner: &PgPool) -> Result<SplitRoles, SplitBootError> {
    role_split_preflight(owner, store.pool()).await?;
    let aligned = store.migrate_as(owner).await?;
    if aligned > 0 {
        tracing::info!(
            aligned,
            "stored the status v0.5.1 served on workareas whose column disagreed, before 0017"
        );
    }
    role_split_postflight(owner, store.pool()).await?;
    Ok(SplitRoles {
        owner: current_user(owner).await?,
        serving: current_user(store.pool()).await?,
    })
}

async fn current_user(pool: &PgPool) -> Result<String, SplitBootError> {
    sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(pool)
        .await
        .map_err(SplitBootError::Login)
}

// ---------------------------------------------------------------------------
// kms-file keyring (ADR-0023 d4: a 0600 file, refused when looser)
// ---------------------------------------------------------------------------

/// The deployment signing key in a 0600 hex file, created on first start.
#[derive(Debug, Clone)]
pub struct FileKeyring {
    path: PathBuf,
}

impl FileKeyring {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn read(&self) -> Result<SigningKey, KeyringError> {
        let meta = std::fs::metadata(&self.path).map_err(|e| KeyringError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                return Err(KeyringError::Permissions(format!(
                    "{} has mode {:o}",
                    self.path.display(),
                    mode & 0o777
                )));
            }
        }
        #[cfg(not(unix))]
        let _ = &meta;
        let text =
            std::fs::read_to_string(&self.path).map_err(|e| KeyringError::Io(e.to_string()))?;
        let raw = hex_decode32(text.trim())
            .ok_or_else(|| KeyringError::Corrupt("not 64 hex characters".to_owned()))?;
        Ok(SigningKey::from_bytes(&raw))
    }

    fn create(&self) -> Result<SigningKey, KeyringError> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| KeyringError::Io(e.to_string()))?;
        }
        let mut seed = [0_u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| KeyringError::Io(e.to_string()))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&self.path) {
            Ok(mut file) => {
                use std::io::Write as _;
                let mut text = hex_encode(&seed);
                text.push('\n');
                file.write_all(text.as_bytes())
                    .map_err(|e| KeyringError::Io(e.to_string()))?;
                Ok(SigningKey::from_bytes(&seed))
            }
            // A concurrent first start won the create; read what it minted.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => self.read(),
            Err(e) => Err(KeyringError::Io(e.to_string())),
        }
    }
}

impl KeyringStore for FileKeyring {
    async fn load_or_create(&self) -> Result<SigningKey, KeyringError> {
        if self.path.exists() {
            self.read()
        } else {
            self.create()
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn hex_decode32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out[i] = u8::try_from(hi * 16 + lo).ok()?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Runtime port adapters
// ---------------------------------------------------------------------------

/// The signer port over the keyring-loaded deployment key — composed by the
/// lead only (ADR-0023 d13; workspace-plan.md ninth trait).
pub struct LeadSigner(SigningKey);

impl LeadSigner {
    #[must_use]
    pub fn new(key: SigningKey) -> Self {
        Self(key)
    }

    /// The raw 32-byte ed25519 public key as 64 hex characters — public
    /// material an operator hands to verifiers out of band (chain-format.md
    /// "How the public key reaches an export recipient"; no API endpoint
    /// serves it in the PoC).
    #[must_use]
    pub fn public_key_hex(&self) -> String {
        hex_encode(&self.0.verifying_key().to_bytes())
    }
}

impl Signer for LeadSigner {
    fn sign(&self, signed_bytes: &[u8]) -> Signature {
        self.0.sign(signed_bytes)
    }

    fn verifying_key(&self) -> VerifyingKey {
        self.0.verifying_key()
    }
}

/// Wall-clock time as the fixed-width signed form (chain-format.md).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> EventTime {
        system_event_time()
    }
}

/// OS-randomness identity: uuids and 256-bit key material.
pub struct OsIds;

impl IdGen for OsIds {
    fn new_id(&self) -> Uuid {
        let mut bytes = [0_u8; 16];
        getrandom::getrandom(&mut bytes).expect("the OS random source is available");
        uuid::Builder::from_random_bytes(bytes).into_uuid()
    }

    fn new_secret(&self) -> [u8; 32] {
        let mut bytes = [0_u8; 32];
        getrandom::getrandom(&mut bytes).expect("the OS random source is available");
        bytes
    }
}

/// The lead's rule set, one per process: every app the lead composes scans
/// with it, and the metrics listener reads its counts (ADR-0065 d11). The
/// personal-data rules stay off (d6).
static RULES: std::sync::LazyLock<oml_scan::RuleSet> =
    std::sync::LazyLock::new(|| oml_scan::RuleSet::new(oml_scan::Options::default()));

/// The deterministic scanner the lead composes: the compiled `oml-scan/1`
/// rule set (ADR-0065), rules, not a model — same input, same verdict
/// (admission.md). A submission's body is reported as `/body`; an anchor
/// value as `/anchor`, its field named by the refusal itself (ADR-0057 d2).
/// The PoC's `[oml:veto]` and `[oml:hold]` markers stay as rules (d10).
pub struct RuleScanner;

/// Above this many bytes a scan leaves the async worker: a mebibyte takes
/// about 13 ms in a release build, and a worker held that long stalls every
/// request queued on it (ADR-0065 d9).
pub const BLOCKING_SCAN_BYTES: usize = 64 * 1024;

/// Run a scan of `len` bytes, off the async worker when it is long and the
/// runtime has other workers to carry the rest (`block_in_place` on a
/// multi-thread runtime); in place otherwise.
fn off_the_executor<T>(len: usize, scan: impl FnOnce() -> T) -> T {
    let multi = tokio::runtime::Handle::try_current()
        .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    if len > BLOCKING_SCAN_BYTES && multi {
        tokio::task::block_in_place(scan)
    } else {
        scan()
    }
}

fn scanned_field(envelope: &AdmissionEnvelope<'_>) -> &'static str {
    match envelope.kind {
        "anchor" => "/anchor",
        "claim_key" => "/claim_key",
        "reason" => "/reason",
        _ => "/body",
    }
}

impl Scanner for RuleScanner {
    fn scan(&self, envelope: &AdmissionEnvelope<'_>) -> ScanOutcome {
        let field = scanned_field(envelope);
        let verdict = off_the_executor(envelope.body.len(), || {
            RULES.scan(&[(field, envelope.body)])
        });
        match (&verdict, verdict.reason()) {
            (oml_scan::Verdict::Veto(_), Some(reason)) => ScanOutcome::Veto(reason),
            (oml_scan::Verdict::Hold(_), Some(reason)) => ScanOutcome::Hold(reason),
            _ => ScanOutcome::Pass,
        }
    }

    fn version(&self) -> Option<&'static str> {
        Some(oml_scan::RULESET)
    }

    fn findings(&self, envelope: &AdmissionEnvelope<'_>) -> Vec<oml_app::ports::ScanFinding> {
        off_the_executor(envelope.body.len(), || {
            RULES.findings(scanned_field(envelope), envelope.body)
        })
        .into_iter()
        .map(|f| oml_app::ports::ScanFinding {
            class: f.class.to_owned(),
            rule: f.rule.to_owned(),
            veto: f.action == oml_scan::Action::Veto,
            at: f.location(),
        })
        .collect()
    }

    fn info(&self) -> Option<oml_app::ports::ScannerInfo> {
        Some(oml_app::ports::ScannerInfo {
            version: oml_scan::RULESET,
            rules: RULES.rules(),
            pii: RULES.options().pii,
        })
    }
}

/// The findings the lead's scanner counted since start, by class, rule and
/// action: what `oml_scan_findings_total` serves (ADR-0065 d11). Counts
/// only; no finding carries what it matched.
#[must_use]
pub fn scan_counts() -> Vec<(&'static str, &'static str, &'static str, u64)> {
    RULES
        .counts()
        .into_iter()
        .map(|(class, rule, action, n)| (class, rule, action.as_str(), n))
        .collect()
}

// ---------------------------------------------------------------------------
// Startup: tenant enumeration, fork check, enrollment token
// ---------------------------------------------------------------------------

/// Every tenant id, under the 0003 startup-enumeration policy (the ordinary
/// RLS context names one tenant; the startup sweep must see them all).
pub async fn enumerate_tenants(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    rows.iter()
        .map(|r| r.try_get("id"))
        .collect::<Result<_, _>>()
}

/// The ADR-0002 d7 startup fork check: every stored chain verifies against
/// its stored head under the deployment key, or the lead refuses writes.
pub async fn startup_fork_check(
    store: &PgStore,
    verifying_key: VerifyingKey,
    tenants: &[Uuid],
) -> Result<(), String> {
    let keys = KeySet::new(vec![verifying_key]);
    let mut failures = Vec::new();
    for tenant_id in tenants {
        match store.verify_stored_chain(*tenant_id, &keys).await {
            Ok(_) => {}
            Err(ForkCheckError::EmptyChain) => {
                failures.push(format!("tenant {tenant_id}: tenants row with no chain"));
            }
            Err(e) => failures.push(format!("tenant {tenant_id}: {e}")),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// The startup names check: the store holds no row that predates names, or
/// the failure names `names_assigned` and each table that holds one. The
/// lead's start runs it after its naming, and alone when the fork check
/// failed, since the naming signs ([`start_lead`]); a lead that fails it
/// serves no request (`/readyz` reports `names_assigned`, every other route
/// answers `system.unavailable`) rather than failing request by request on
/// the first row with no name (ADR-0026 d11). A store this build
/// bootstrapped passes, and a store the check cannot read fails it.
pub async fn startup_names_check(store: &PgStore, tenants: &[Uuid]) -> Result<(), String> {
    match store.rows_without_names(tenants).await {
        Ok(rows) if rows.is_empty() => Ok(()),
        Ok(rows) => {
            let rows: Vec<String> = rows
                .iter()
                .map(|(table, n)| format!("{table} {n}"))
                .collect();
            Err(format!(
                "names_assigned: rows that predate names ({}), which the lead names at its \
                 start, before it serves (ADR-0026 d11)",
                rows.join(", ")
            ))
        }
        Err(e) => Err(format!("names_assigned: the store could not be read: {e}")),
    }
}

/// The lead's naming at its start, before it serves: every row of
/// `tenants` that predates names is named under rules 1–5 of http-api.md
/// "Rows created before names", in one `system.names_assigned` per chain
/// that has any, and then the startup names check reads that none is left,
/// so a chain the naming refuses, or a row it leaves, fails the start's
/// names by name. A chain with nothing to name journals nothing, so every
/// later start journals nothing (ADR-0026 d11, d15).
async fn startup_names(
    app: &LeadApp,
    store: &PgStore,
    tenants: &[Uuid],
) -> Result<Vec<AssignedNames>, String> {
    let assigned = app
        .name_rows_that_predate_names(tenants)
        .await
        .map_err(|e| format!("names_assigned: {e}"))?;
    startup_names_check(store, tenants).await?;
    Ok(assigned)
}

/// The single-use enrollment token minted against an empty database
/// (deployment.md bootstrap; the PoC lead prints it to stdout).
#[must_use]
pub fn mint_enrollment_token() -> String {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes).expect("the OS random source is available");
    format!("enr_{}", hex_encode(&bytes))
}

// ---------------------------------------------------------------------------
// The connections' operational removal
// ---------------------------------------------------------------------------

/// How often [`spawn_connection_sweep`] runs [`sweep_connections`]: an hour,
/// so a registration that never connected is gone at most an hour past its
/// 24 hours, and a lapsed row at most an hour past its expiry (ADR-0031 d5,
/// d9, d10).
pub const CONNECTION_SWEEP_SECONDS: u64 = 60 * 60;

/// What one [`sweep_connections`] removed, summed over the tenants it saw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnectionSweep {
    /// The tenants the sweep enumerated and swept.
    pub tenants: usize,
    /// Registrations that had not connected 24 hours after they registered.
    pub registrations_removed: u64,
    /// `authorized` connections whose authorization code lapsed unexchanged.
    pub connections_removed: u64,
    /// Token rows past their own expiry, or of a revoked connection.
    pub tokens_removed: u64,
    /// Revoked connections whose `auth_code_hash` was cleared.
    pub codes_cleared: u64,
    /// Authorization requests 24 hours past their expiry.
    pub authorizations_removed: u64,
    /// Web sessions past their expiry (ADR-0045 d5).
    pub web_sessions_removed: u64,
}

/// The operational removal at `at` (postgres-schema.md, connections): every
/// registration that has not connected and registered 24 hours or more
/// before `at`, then, for every tenant the store holds now, the store's
/// [`sweep_expired`](oml_app::ports::ConnectionStore::sweep_expired). It
/// journals nothing (ADR-0031 d5, d9, d10, d12). A failed enumeration or
/// removal fails the whole sweep with its reason; what an earlier step
/// removed stays removed, and the next sweep repeats the rest.
pub async fn sweep_connections(store: &PgStore, at: &EventTime) -> Result<ConnectionSweep, String> {
    use oml_app::domain::oauth::UNUSED_REGISTRATION_SECONDS;
    use oml_app::ports::ConnectionStore as _;

    let registered_by = EventTime::from_unix_micros(
        at.unix_micros() - i64::from(UNUSED_REGISTRATION_SECONDS) * 1_000_000,
    )
    .ok_or_else(|| format!("{} less 24 hours is not an event time", at.as_str()))?;
    let registrations_removed = store
        .remove_unused_dcr(&registered_by)
        .await
        .map_err(|e| format!("unused registrations: {e}"))?;
    let tenants = enumerate_tenants(store.pool())
        .await
        .map_err(|e| format!("tenant enumeration: {e}"))?;
    let mut sweep = ConnectionSweep {
        tenants: tenants.len(),
        registrations_removed,
        ..ConnectionSweep::default()
    };
    for tenant_id in tenants {
        let report = store
            .sweep_expired(tenant_id, at)
            .await
            .map_err(|e| format!("tenant {tenant_id}: {e}"))?;
        sweep.connections_removed += report.connections_removed;
        sweep.tokens_removed += report.tokens_removed;
        sweep.codes_cleared += report.codes_cleared;
        sweep.authorizations_removed += report.authorizations_removed;
        sweep.web_sessions_removed +=
            oml_app::ports::WebStore::web_sessions_sweep(store, tenant_id, at)
                .await
                .map_err(|e| format!("tenant {tenant_id} web sessions: {e}"))?;
    }
    Ok(sweep)
}

/// The `event` field of the log line each sweep writes (observability.md).
pub const EVENT_CONNECTION_SWEEP: &str = "connection_sweep";
/// The `event` field of the log line a failed sweep writes.
pub const EVENT_CONNECTION_SWEEP_FAILED: &str = "connection_sweep_failed";

/// Runs [`sweep_connections`] at the lead's start and every
/// [`CONNECTION_SWEEP_SECONDS`] after, on the system clock, logging
/// [`EVENT_CONNECTION_SWEEP`] with its counts or
/// [`EVENT_CONNECTION_SWEEP_FAILED`] with its reason. A failed sweep never
/// stops the lead or the next sweep.
pub fn spawn_connection_sweep(store: PgStore) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticks =
            tokio::time::interval(std::time::Duration::from_secs(CONNECTION_SWEEP_SECONDS));
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            match sweep_connections(&store, &SystemClock.now()).await {
                Ok(sweep) => tracing::info!(
                    event = EVENT_CONNECTION_SWEEP,
                    tenants = sweep.tenants,
                    registrations_removed = sweep.registrations_removed,
                    connections_removed = sweep.connections_removed,
                    tokens_removed = sweep.tokens_removed,
                    codes_cleared = sweep.codes_cleared,
                    authorizations_removed = sweep.authorizations_removed,
                    web_sessions_removed = sweep.web_sessions_removed,
                    "connections swept (ADR-0031 d5, d9, d10)"
                ),
                Err(reason) => tracing::error!(
                    event = EVENT_CONNECTION_SWEEP_FAILED,
                    %reason,
                    "CONNECTION SWEEP FAILED — the next sweep retries"
                ),
            }
        }
    })
}

// ---------------------------------------------------------------------------
// The composed state
// ---------------------------------------------------------------------------

/// The store the lead composes: the Postgres adapter, every port call timed
/// into the running request (ADR-0042 d14).
pub type LeadStore = Timed<PgStore>;
pub type LeadApp = App<LeadStore, LeadSigner, SystemClock, OsIds, RuleScanner>;
pub type LeadState = ApiState<LeadStore, LeadSigner, SystemClock, OsIds, RuleScanner>;

/// A `/readyz` probe over the composition's own pool.
#[must_use]
pub fn pg_ping(pool: PgPool) -> DbPing {
    Arc::new(move || {
        let pool = pool.clone();
        Box::pin(async move { sqlx::query("SELECT 1").execute(&pool).await.is_ok() })
    })
}

/// The lead's app: one store handle and the signer, which exists only here
/// (ADR-0023 d13).
#[must_use]
pub fn lead_app(store: &PgStore, signing_key: SigningKey) -> LeadApp {
    // The lead's default search where the store can run it; a store probed
    // before 0020 created `pg_trgm` serves the app's keyword pipelines. The
    // start sets the configured search after refusing what the store cannot
    // run (ADR-0049 d8).
    let search = default_search();
    let search = if search.require_supported(&store.retrievers()).is_ok() {
        search
    } else {
        SearchConfig::default()
    };
    App::new(
        Timed::new(store.clone()),
        LeadSigner::new(signing_key),
        SystemClock,
        OsIds,
        RuleScanner,
    )
    .with_observer(Arc::new(ScopeObserver))
    .with_search(search)
}

/// The lead composition: one store handle drives the app and the transport's
/// head reads; the signer exists only here (ADR-0023 d13).
#[must_use]
pub fn lead_state(
    store: &PgStore,
    signing_key: SigningKey,
    db_ping: DbPing,
    fork_check_ok: bool,
    enrollment_token: Option<String>,
) -> LeadState {
    composed(
        lead_app(store, signing_key),
        store,
        db_ping,
        fork_check_ok,
        enrollment_token,
        PinnedFetcher,
        Traffic::default(),
    )
}

/// The state over `app`, its authorize page reading client documents
/// through `fetcher` behind the one [`CappedFetcher`] the lead composes:
/// the 16 slots, one per host name and one per resolved address, and the
/// address allow-list before any connection (ADR-0031 d5).
fn composed<F: DocumentFetcher + 'static>(
    app: LeadApp,
    store: &PgStore,
    db_ping: DbPing,
    fork_check_ok: bool,
    enrollment_token: Option<String>,
    fetcher: F,
    traffic: Traffic,
) -> LeadState {
    ApiState::new(
        Arc::new(app),
        Timed::new(store.clone()),
        db_ping,
        fork_check_ok,
        enrollment_token,
    )
    .with_build(build_info())
    .with_document_fetcher(CappedFetcher::new(fetcher))
    .with_traffic(traffic)
}

/// What the lead's start decided before it serves: the boot order after the
/// keyring, per deployment.md and ADR-0002 d7 — the tenants, the fork check,
/// the names, the enrollment token and the router built on them.
pub struct LeadStart {
    /// Every tenant the start found.
    pub tenants: Vec<Uuid>,
    /// The startup fork check: on a failure the router refuses writes and
    /// `/readyz` answers 503 (ADR-0002 d7).
    pub fork_check: Result<(), String>,
    /// The chains the start named, one `system.names_assigned` each, or why
    /// a row is left with no name, in which case the router serves no
    /// request (ADR-0026 d11).
    pub names: Result<Vec<AssignedNames>, String>,
    /// The single-use enrollment token, minted against an empty deployment
    /// whose fork check passed (deployment.md bootstrap).
    pub enrollment_token: Option<String>,
    pub router: Router,
}

/// The lead's start over a migrated store, `main`'s and the tests' alike:
/// enumerate the tenants, run the fork check, then name every row that
/// predates names, mint the enrollment token against an empty deployment,
/// and build the router on what they found, with the OAuth routes where
/// `oauth_issuer` is set (ADR-0031 d1), its authorize page fetching client
/// documents over the network through [`PinnedFetcher`] (ADR-0031 d5).
/// Only a failed tenant enumeration fails the start.
pub async fn start_lead(
    store: &PgStore,
    signing_key: SigningKey,
    oauth_issuer: Option<Issuer>,
) -> Result<LeadStart, String> {
    start_lead_with(
        store,
        signing_key,
        oauth_issuer,
        PinnedFetcher,
        Traffic::default(),
        None,
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
    )
    .await
}

/// [`start_lead`] with `fetcher` as the outbound adapter under the fetch
/// caps in place of [`PinnedFetcher`], so a test sees what the composed
/// page asks of the network without reaching it, and the request line
/// configured by `traffic` (ADR-0042 d7), as `main` starts it.
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per setting main composes"
)]
pub async fn start_lead_with<F: DocumentFetcher + 'static>(
    store: &PgStore,
    signing_key: SigningKey,
    oauth_issuer: Option<Issuer>,
    fetcher: F,
    traffic: Traffic,
    web: Option<WebPages>,
    brief_max_tokens: u64,
    search: Option<SearchConfig>,
    reranker: Option<Arc<dyn oml_app::ports::Reranker>>,
) -> Result<LeadStart, String> {
    start_lead_with_models(
        store,
        signing_key,
        oauth_issuer,
        fetcher,
        traffic,
        web,
        brief_max_tokens,
        search,
        reranker,
        None,
    )
    .await
}

/// [`start_lead_with`], with the learned sparse encoder the sparse
/// pipelines read (ADR-0052 d16): a configuration serving one without it
/// stops the start, naming the setting.
///
/// # Errors
///
/// As [`start_lead_with`].
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per setting main composes"
)]
pub async fn start_lead_with_models<F: DocumentFetcher + 'static>(
    store: &PgStore,
    signing_key: SigningKey,
    oauth_issuer: Option<Issuer>,
    fetcher: F,
    traffic: Traffic,
    web: Option<WebPages>,
    brief_max_tokens: u64,
    search: Option<SearchConfig>,
    reranker: Option<Arc<dyn oml_app::ports::Reranker>>,
    sparse: Option<Arc<dyn oml_app::ports::SparseEncoder>>,
) -> Result<LeadStart, String> {
    // The retrievers the database can run decide which pipelines may be
    // served: a configuration naming one it cannot run stops the start,
    // naming the pipeline and what is missing (ADR-0049 d8).
    let store = &store
        .clone()
        .with_search_probe()
        .await
        .map_err(|e| format!("search probe: {e}"))?;
    let search = search.unwrap_or_else(default_search);
    search
        .require_supported(&store.retrievers())
        .map_err(|e| e.to_string())?;
    // A reranking pipeline is served only with its model loaded (ADR-0053
    // d8).
    search
        .require_reranker(reranker.is_some())
        .map_err(|e| e.to_string())?;
    // A sparse pipeline is served only with its encoder loaded, never by
    // another pipeline in its name (ADR-0052 d16).
    search
        .require_sparse(sparse.is_some())
        .map_err(|e| e.to_string())?;
    // A lead serving no sparse pipeline turns every tenant's sparse queue
    // off and empties it, so nothing is queued for an encoder no one runs;
    // one serving one turns them on through its encoder's backfill
    // (ADR-0052 d15, HIGH-1 on review).
    if !search.needs_sparse() {
        let off = store
            .sparse_off()
            .await
            .map_err(|e| format!("search.sparse: turning the queue off: {e}"))?;
        if off > 0 {
            tracing::info!(
                event = "sparse_queue_off",
                tenants = off,
                "no sparse pipeline is served: the learned sparse queue is off and empty"
            );
        }
    }
    let tenants = enumerate_tenants(store.pool())
        .await
        .map_err(|e| format!("tenant enumeration: {e}"))?;
    let fork_check = startup_fork_check(store, signing_key.verifying_key(), &tenants).await;
    let app = lead_app(store, signing_key)
        .with_brief_max_tokens(brief_max_tokens)
        .with_search(search);
    let app = match reranker {
        Some(reranker) => app.with_reranker(reranker),
        None => app,
    };
    // Each recall's event, and what its reader did next, go to a bounded
    // queue a task writes off the recall's path, counted on the metrics
    // listener (ADR-0054 d1, d2).
    let meter: Arc<dyn oml_app::ports::TelemetryMeter> = Arc::clone(traffic.metrics()) as _;
    // The scan's findings by rule, read at each scrape (ADR-0065 d11).
    traffic.metrics().with_scan_counts(scan_counts);
    let (telemetry, _writer) = PgTelemetry::spawn(store.pool().clone(), meter);
    let app = app.with_telemetry(telemetry);
    let app = match sparse {
        Some(sparse) => app.with_sparse_encoder(sparse),
        None => app,
    };
    let app = match oauth_issuer {
        Some(issuer) => app.with_oauth_issuer(issuer),
        None => app,
    };
    // The naming signs, and the lead never signs on top of a fork (ADR-0002
    // d7): after a failed fork check the start only reads whether a row has
    // no name.
    let names = if fork_check.is_ok() {
        startup_names(&app, store, &tenants).await
    } else {
        startup_names_check(store, &tenants)
            .await
            .map(|()| Vec::new())
    };
    let enrollment_token = (tenants.is_empty() && fork_check.is_ok()).then(mint_enrollment_token);
    let state = composed(
        app,
        store,
        pg_ping(store.pool().clone()),
        fork_check.is_ok(),
        enrollment_token.clone(),
        fetcher,
        traffic,
    );
    let state = match web {
        Some(pages) => state.with_web(pages),
        None => state,
    };
    let router = state.with_names_assigned(names.is_ok()).into_router();
    Ok(LeadStart {
        tenants,
        fork_check,
        names,
        enrollment_token,
        router,
    })
}

/// The lead's subscriber: every line `tracing-subscriber`'s JSON format,
/// `timestamp` (RFC 3339, UTC), `level`, `target`, then the event's fields
/// at the top level, and no span fields, on `writer`, which `main` sets to
/// stderr: stdout is the show-once enrollment token's (ADR-0042 d1).
pub fn log_subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    exporting_subscriber(writer, None)
}

/// [`log_subscriber`], and while trace export is on, every span exported
/// through `tracer` as well (ADR-0042 d13).
pub fn exporting_subscriber<W>(
    writer: W,
    tracer: Option<opentelemetry_sdk::trace::Tracer>,
) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    use tracing_subscriber::layer::SubscriberExt as _;
    let lines = tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(false)
        .with_writer(writer);
    let spans = tracer.map(|t| tracing_opentelemetry::layer().with_tracer(t));
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(lines)
        .with(spans)
}

/// This binary's build, as `build.rs` captured it (http-api.md "Health").
#[must_use]
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        commit: env!("OML_BUILD_COMMIT").to_owned(),
        commit_time: env!("OML_BUILD_COMMIT_TIME").to_owned(),
        build_time: env!("OML_BUILD_TIME").to_owned(),
        rustc: env!("OML_BUILD_RUSTC").to_owned(),
        target: env!("OML_BUILD_TARGET").to_owned(),
        profile: env!("OML_BUILD_PROFILE").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn env_names_are_mechanical() {
        assert_eq!(env_name("postgres.url"), "OML_POSTGRES__URL");
        assert_eq!(env_name("server.listen"), "OML_SERVER__LISTEN");
        assert_eq!(
            env_name("uploads.max_attachment_size"),
            "OML_UPLOADS__MAX_ATTACHMENT_SIZE"
        );
    }

    #[test]
    fn precedence_is_defaults_file_env() {
        let file = "[postgres]\nurl = \"postgres://file/db\"\nowner_url = \"postgres://owner@file/db\"\n[server]\nlisten = \"0.0.0.0:9999\"\n";
        // file over defaults
        let cfg = Config::from_sources(Some(file), &|_| None).expect("file config loads");
        assert_eq!(cfg.postgres_url, "postgres://file/db");
        assert_eq!(cfg.listen, "0.0.0.0:9999");
        assert_eq!(cfg.keyring_path, PathBuf::from("oml-keyring")); // default
        // env over file
        let env = env_of(&[("OML_POSTGRES__URL", "postgres://env/db")]);
        let cfg = Config::from_sources(Some(file), &|k| env.get(k).cloned())
            .expect("env-over-file config loads");
        assert_eq!(cfg.postgres_url, "postgres://env/db");
        assert_eq!(cfg.listen, "0.0.0.0:9999");
    }

    /// `search.enabled` and `search.default` default to the gate's choice,
    /// read a TOML array or a comma list, and stop the start on an unknown
    /// name or a default outside the set (ADR-0049 d4, d8).
    #[test]
    fn search_settings_default_to_the_gated_pipeline_and_refuse_what_is_not_served() {
        let base = [
            ("OML_POSTGRES__URL", "postgres://env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let load = |extra: &[(&str, &str)]| {
            let env = env_of(&[base.as_slice(), extra].concat());
            Config::from_sources(None, &|k| env.get(k).cloned())
        };
        let cfg = load(&[]).expect("defaults load");
        assert_eq!(cfg.search, default_search());
        assert_eq!(cfg.search.default_pipeline(), Pipeline::Bm25Postings);
        assert!(
            cfg.search.serves(Pipeline::Bm25),
            "bm25 stays served: the postings' fallback and comparison"
        );
        assert!(
            !cfg.search.serves(Pipeline::KeywordAll),
            "keyword-all is opt-in"
        );
        assert!(
            !cfg.search.serves(Pipeline::Bm25Trigram),
            "bm25-trigram, which needs pg_trgm, is opt-in"
        );
        let cfg = load(&[
            ("OML_SEARCH__ENABLED", "keyword-all, keyword-any"),
            ("OML_SEARCH__DEFAULT", "keyword-any"),
        ])
        .expect("a comma list loads");
        assert_eq!(
            cfg.search.enabled(),
            [Pipeline::KeywordAll, Pipeline::KeywordAny]
        );
        let file = "[postgres]\nurl = \"postgres://f/db\"\nowner_url = \"postgres://o@f/db\"\n[search]\nenabled = [\"bm25\"]\ndefault = \"bm25\"\n";
        let cfg = Config::from_sources(Some(file), &|_| None).expect("a TOML array loads");
        assert_eq!(cfg.search.enabled(), [Pipeline::Bm25]);
        for (extra, says) in [
            (&[("OML_SEARCH__ENABLED", "hybrid")][..], "hybrid"),
            (
                &[("OML_SEARCH__ENABLED", "keyword-any")][..],
                "search.default bm25",
            ),
            (&[("OML_SEARCH__DEFAULT", "dense")][..], "dense"),
            (&[("OML_SEARCH__ENABLED", " , ")][..], "empty"),
        ] {
            let err = load(extra).expect_err("refused");
            assert!(err.to_string().contains(says), "{err}");
        }
    }

    /// `search.rerank.*` default to the measured choice, floor off, read the
    /// file's floats and booleans as the environment's strings, and stop the
    /// start on a model this build does not pin or a value out of range
    /// (ADR-0053 d2-d6).
    #[test]
    fn rerank_settings_default_to_the_measured_model_with_the_floor_off_and_refuse_the_unknown() {
        let base = [
            ("OML_POSTGRES__URL", "postgres://env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let load = |extra: &[(&str, &str)]| {
            let env = env_of(&[base.as_slice(), extra].concat());
            Config::from_sources(None, &|k| env.get(k).cloned())
        };
        let cfg = load(&[]).expect("defaults load");
        assert_eq!(
            cfg.rerank.model_dir, None,
            "no state directory named, none assumed"
        );
        assert_eq!(cfg.rerank.model, oml_rerank::MINILM_L6);
        assert!(!cfg.rerank.fetch, "no network at start unless asked");
        assert_eq!(
            cfg.search.rerank().floor,
            None,
            "the floor is off by default"
        );
        assert!(
            !cfg.search.serves(Pipeline::Bm25Rerank),
            "bm25-rerank is opt-in"
        );
        let file = "[postgres]\nurl = \"postgres://f/db\"\nowner_url = \"postgres://o@f/db\"\n[search]\nenabled = [\"bm25\", \"bm25-rerank\"]\ndefault = \"bm25\"\n[search.rerank]\nmodel = \"ms-marco-MiniLM-L4-v2\"\nmodel_dir = \"/var/lib/oml/l4\"\nfetch = true\ndepth = 10\nfloor = 0.005\nmax_concurrent = 3\ntimeout_ms = 750\n";
        let cfg = Config::from_sources(Some(file), &|_| None).expect("the file loads");
        assert_eq!(cfg.rerank.model, oml_rerank::MINILM_L4);
        assert_eq!(cfg.rerank.model_dir, Some(PathBuf::from("/var/lib/oml/l4")));
        assert!(cfg.rerank.fetch);
        assert_eq!(cfg.rerank.limits.max_concurrent, 3);
        assert_eq!(
            cfg.rerank.limits.timeout,
            std::time::Duration::from_millis(750)
        );
        assert_eq!(cfg.search.rerank().depth, 10);
        assert_eq!(cfg.search.rerank().floor, Some(0.005));
        assert!(cfg.search.needs_reranker());
        let off = load(&[("OML_SEARCH__RERANK__FLOOR", "off")]).expect("off loads");
        assert_eq!(off.search.rerank().floor, None);
        for (extra, says) in [
            (
                &[("OML_SEARCH__RERANK__MODEL", "jina-reranker-v2")][..],
                "jina-reranker-v2",
            ),
            (
                &[("OML_SEARCH__RERANK__DEPTH", "0")][..],
                "search.rerank.depth",
            ),
            (&[("OML_SEARCH__RERANK__DEPTH", "101")][..], "depth"),
            (&[("OML_SEARCH__RERANK__FLOOR", "1.5")][..], "floor"),
            (
                &[("OML_SEARCH__RERANK__FETCH", "yes")][..],
                "search.rerank.fetch",
            ),
            (
                &[("OML_SEARCH__RERANK__TIMEOUT_MS", "0")][..],
                "search.rerank.timeout_ms",
            ),
            (&[("OML_SEARCH__TIMINGS", "yes")][..], "search.timings"),
        ] {
            let err = load(extra).expect_err("refused");
            assert!(err.to_string().contains(says), "{err}");
        }
    }

    /// `search.rerank.model_dir` is absolute: unset, it lies under the
    /// process's state directory (systemd's `STATE_DIRECTORY`, else
    /// `XDG_STATE_HOME/oml`, else `~/.local/state/oml`), never under the
    /// working directory, and a relative value stops the start; the threads
    /// and the token limit default to the measured values and refuse what is
    /// out of range (ADR-0053 d2, d3, d5).
    #[test]
    fn the_model_dir_is_absolute_and_defaults_under_the_state_directory() {
        let base = [
            ("OML_POSTGRES__URL", "postgres://env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let load = |extra: &[(&str, &str)]| {
            let env = env_of(&[base.as_slice(), extra].concat());
            Config::from_sources(None, &|k| env.get(k).cloned())
        };
        let under = |extra: &[(&str, &str)]| load(extra).expect("loads").rerank.model_dir;
        let l6 = oml_rerank::MINILM_L6.name;
        assert_eq!(
            under(&[("STATE_DIRECTORY", "/var/lib/oml"), ("HOME", "/home/u")]),
            Some(PathBuf::from(format!("/var/lib/oml/models/{l6}")))
        );
        assert_eq!(
            under(&[("XDG_STATE_HOME", "/x/state"), ("HOME", "/home/u")]),
            Some(PathBuf::from(format!("/x/state/oml/models/{l6}")))
        );
        assert_eq!(
            under(&[("HOME", "/home/u")]),
            Some(PathBuf::from(format!(
                "/home/u/.local/state/oml/models/{l6}"
            )))
        );
        assert_eq!(
            under(&[("HOME", "relative")]),
            None,
            "a relative home is no anchor"
        );
        let err = load(&[("OML_SEARCH__RERANK__MODEL_DIR", "oml-models/l6")])
            .expect_err("a relative model_dir is refused");
        assert!(err.to_string().contains("absolute"), "{err}");

        let cfg = load(&[]).expect("defaults load");
        assert_eq!(cfg.rerank.limits.threads, DEFAULT_RERANK_THREADS);
        assert_eq!(cfg.rerank.limits.max_tokens, DEFAULT_RERANK_MAX_TOKENS);
        let cfg = load(&[
            ("OML_SEARCH__RERANK__THREADS", "4"),
            ("OML_SEARCH__RERANK__MAX_TOKENS", "512"),
        ])
        .expect("loads");
        assert_eq!(cfg.rerank.limits.threads, 4);
        assert_eq!(cfg.rerank.limits.max_tokens, 512);
        for (extra, says) in [
            (
                &[("OML_SEARCH__RERANK__THREADS", "0")][..],
                "search.rerank.threads",
            ),
            (
                &[("OML_SEARCH__RERANK__MAX_TOKENS", "513")][..],
                "search.rerank.max_tokens",
            ),
            (
                &[("OML_SEARCH__RERANK__MAX_TOKENS", "32")][..],
                "search.rerank.max_tokens",
            ),
        ] {
            let err = load(extra).expect_err("refused");
            assert!(err.to_string().contains(says), "{err}");
        }
    }

    /// A configuration serving no reranking pipeline loads no model and
    /// reads no file; one serving `bm25-rerank` with the model absent is
    /// refused naming the file and the remedy, and fetches only when
    /// `search.rerank.fetch` allows it (ADR-0053 d3, d8).
    #[test]
    fn a_reranking_pipeline_without_its_model_stops_the_start_naming_the_file() {
        let dir = std::env::temp_dir().join(format!("oml-rerank-absent-{}", std::process::id()));
        let settings = RerankSettings {
            model_dir: Some(dir.clone()),
            ..RerankSettings::default()
        };
        let plain = default_search();
        assert!(
            load_reranker(&plain, &settings, |_| panic!("no fetch"))
                .expect("nothing to load")
                .is_none()
        );
        let reranking =
            SearchConfig::new(&[Pipeline::Bm25, Pipeline::Bm25Rerank], Pipeline::Bm25).unwrap();
        let err = load_reranker(&reranking, &settings, |_| panic!("fetch is off"))
            .err()
            .expect("refused");
        assert!(err.contains("config.json is missing"), "{err}");
        assert!(err.contains("search.rerank.fetch"), "{err}");
        let fetching = RerankSettings {
            fetch: true,
            ..settings
        };
        let err = load_reranker(&reranking, &fetching, |_| Err("offline".to_owned()))
            .err()
            .expect("refused");
        assert!(err.contains("offline"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A configuration serving no sparse pipeline, the default among them,
    /// loads no sparse model and reads no file, so the lead starts and
    /// serves without the model's files; one serving `sparse` or
    /// `bm25-sparse` with the files absent is refused naming the file and
    /// the remedy, never served by another pipeline (ADR-0052 d16).
    #[test]
    fn a_sparse_pipeline_without_its_model_stops_the_start_and_the_default_needs_none() {
        let dir = std::env::temp_dir().join(format!("oml-sparse-absent-{}", std::process::id()));
        let settings = SparseSettings {
            model_dir: Some(dir.clone()),
            ..SparseSettings::default()
        };
        assert!(
            !default_search().needs_sparse(),
            "the default serves no sparse pipeline"
        );
        assert!(
            load_sparse(&default_search(), &settings, |_| panic!("no fetch"))
                .expect("nothing to load")
                .is_none()
        );
        for pipeline in [
            Pipeline::Sparse,
            Pipeline::Bm25Sparse,
            Pipeline::Bm25SparseRrf,
        ] {
            let sparse =
                SearchConfig::new(&[Pipeline::Bm25Postings, pipeline], Pipeline::Bm25Postings)
                    .unwrap();
            let err = load_sparse(&sparse, &settings, |_| panic!("fetch is off"))
                .err()
                .expect("refused");
            assert!(err.contains("config.json is missing"), "{err}");
            assert!(err.contains("search.sparse.fetch"), "{err}");
            assert_eq!(
                sparse.require_sparse(false),
                Err(oml_app::search::SearchConfigError::NoSparseEncoder(
                    pipeline
                ))
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `search.sparse.*` defaults to no fetch, two threads and the state
    /// directory's model path, and refuses a relative path or a fetch that
    /// is not true or false, naming the key (ADR-0052 d14).
    #[test]
    fn sparse_settings_default_to_no_fetch_and_refuse_a_relative_dir() {
        let base = "[postgres]\nurl = \"postgres://f/db\"\nowner_url = \"postgres://o@f/db\"\n";
        let env = env_of(&[("HOME", "/home/someone")]);
        let cfg = Config::from_sources(Some(base), &|k| env.get(k).cloned()).expect("loads");
        assert!(!cfg.sparse.fetch);
        assert_eq!(cfg.sparse.threads, DEFAULT_SPARSE_THREADS);
        assert_eq!(
            cfg.sparse.model_dir,
            Some(PathBuf::from(
                "/home/someone/.local/state/oml/models/opensearch-neural-sparse-encoding-doc-v3-distill"
            ))
        );
        for (key, value, says) in [
            (
                "OML_SEARCH__SPARSE__MODEL_DIR",
                "models/sparse",
                "search.sparse.model_dir",
            ),
            ("OML_SEARCH__SPARSE__FETCH", "yes", "search.sparse.fetch"),
            ("OML_SEARCH__SPARSE__THREADS", "0", "search.sparse.threads"),
        ] {
            let env = env_of(&[(key, value)]);
            let err =
                Config::from_sources(Some(base), &|k| env.get(k).cloned()).expect_err("refused");
            assert!(err.to_string().contains(says), "{err}");
        }
    }

    #[test]
    fn postgres_url_is_required_and_foreign_kms_refused() {
        assert!(Config::from_sources(None, &|_| None).is_err());
        let env = env_of(&[
            ("OML_POSTGRES__URL", "postgres://env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
            ("OML_KMS__ADAPTER", "aws"),
        ]);
        let err = Config::from_sources(None, &|k| env.get(k).cloned())
            .expect_err("a non-file kms adapter is refused in this composition");
        assert!(matches!(err, ConfigError::Invalid(_)), "got: {err:?}");
        assert!(err.to_string().contains("kms.adapter"), "got: {err}");
    }

    #[test]
    fn postgres_owner_url_is_read_from_file_and_from_oml_postgres_owner_url() {
        let file = "[postgres]\nurl = \"postgres://serving@file/db\"\nowner_url = \"postgres://owner@file/db\"\n";
        let cfg = Config::from_sources(Some(file), &|_| None).expect("file config loads");
        assert_eq!(cfg.postgres_owner_url, "postgres://owner@file/db");
        assert_eq!(cfg.postgres_url, "postgres://serving@file/db");
        let env = env_of(&[("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db")]);
        let cfg = Config::from_sources(Some(file), &|k| env.get(k).cloned())
            .expect("env-over-file config loads");
        assert_eq!(cfg.postgres_owner_url, "postgres://owner@env/db");
        let env = env_of(&[
            ("OML_POSTGRES__URL", "postgres://serving@env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ]);
        let cfg =
            Config::from_sources(None, &|k| env.get(k).cloned()).expect("env-only config loads");
        assert_eq!(cfg.postgres_owner_url, "postgres://owner@env/db");
    }

    #[test]
    fn a_missing_owner_url_refuses_with_postgres_role_split_owner_url_missing() {
        let file = "[postgres]\nurl = \"postgres://serving@file/db\"\n";
        for err in [
            Config::from_sources(Some(file), &|_| None),
            Config::from_sources(None, &|k| {
                (k == "OML_POSTGRES__URL").then(|| "postgres://serving@env/db".to_owned())
            }),
        ]
        .map(|r| r.expect_err("no owner URL is refused (ADR-0034 d5)"))
        {
            assert_eq!(
                err,
                ConfigError::RoleSplit(RoleSplitError {
                    reason: RoleSplitReason::OwnerUrlMissing
                }),
                "got: {err}"
            );
            assert_eq!(err.to_string(), "postgres.role_split: owner_url_missing");
        }
    }

    #[test]
    fn oauth_issuer_is_parsed_as_a_public_origin_and_a_bad_value_is_refused() {
        let base = [
            ("OML_POSTGRES__URL", "postgres://serving@env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let cfg = Config::from_sources(None, &|k| env_of(&base).get(k).cloned())
            .expect("a config without oauth.issuer loads");
        assert_eq!(cfg.oauth_issuer, None);
        let file = "[oauth]\nissuer = \"https://oml.example.com\"\n";
        let cfg = Config::from_sources(Some(file), &|k| env_of(&base).get(k).cloned())
            .expect("a public origin loads");
        assert_eq!(
            cfg.oauth_issuer.as_ref().map(Issuer::as_str),
            Some("https://oml.example.com")
        );
        for bad in ["https://oml.example.com/", "http://oml.example.com"] {
            let mut env = env_of(&base);
            env.insert("OML_OAUTH__ISSUER".to_owned(), bad.to_owned());
            let err = Config::from_sources(Some(file), &|k| env.get(k).cloned())
                .expect_err("not a public origin");
            assert!(matches!(err, ConfigError::Invalid(_)), "{bad}: {err:?}");
            assert!(err.to_string().contains("oauth.issuer"), "{err}");
        }
    }

    #[test]
    fn a_provider_without_its_secret_stops_the_process() {
        let base = [
            ("OML_POSTGRES__URL", "postgres://serving@env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
            ("OML_OAUTH__ISSUER", "https://oml.example.com"),
        ];
        let cfg = Config::from_sources(None, &|k| env_of(&base).get(k).cloned())
            .expect("no provider loads");
        assert!(cfg.web_pages().is_none(), "no provider, no pages");
        for (set, missing) in [
            ("OML_WEB__GOOGLE__CLIENT_ID", "web.google.client_secret"),
            ("OML_WEB__GITHUB__CLIENT_SECRET", "web.github.client_id"),
        ] {
            let mut env = env_of(&base);
            env.insert(set.to_owned(), "value".to_owned());
            let err = Config::from_sources(None, &|k| env.get(k).cloned())
                .expect_err("one value without the other stops the start");
            assert!(err.to_string().contains(missing), "{err}");
        }
        let mut env = env_of(&base);
        env.insert("OML_WEB__GITHUB__CLIENT_ID".to_owned(), "gh-id".to_owned());
        env.insert(
            "OML_WEB__GITHUB__CLIENT_SECRET".to_owned(),
            "gh-s3cret".to_owned(),
        );
        let cfg = Config::from_sources(None, &|k| env.get(k).cloned()).expect("a provider loads");
        assert!(cfg.web_pages().is_some());
        assert!(
            !format!("{cfg:?}").contains("gh-s3cret"),
            "the secret is never printed"
        );
        env.remove("OML_OAUTH__ISSUER");
        let err = Config::from_sources(None, &|k| env.get(k).cloned())
            .expect_err("a provider without oauth.issuer stops the start");
        assert!(err.to_string().contains("oauth.issuer"), "{err}");
    }

    #[test]
    fn trusted_proxies_are_a_toml_array_in_the_file_a_comma_list_in_the_environment_and_a_bad_entry_stops_the_start()
     {
        let base = [
            ("OML_POSTGRES__URL", "postgres://serving@env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let cfg = Config::from_sources(None, &|k| env_of(&base).get(k).cloned())
            .expect("no proxies loads");
        assert_eq!(cfg.trusted_proxies, TrustedProxies::default());
        let file = "[server]\ntrusted_proxies = [\"127.0.0.1/32\", \"::1/128\"]\n";
        let cfg = Config::from_sources(Some(file), &|k| env_of(&base).get(k).cloned())
            .expect("a TOML array loads");
        assert_eq!(
            cfg.trusted_proxies,
            TrustedProxies::parse(&["127.0.0.1/32", "::1/128"]).expect("parses")
        );
        let mut env = env_of(&base);
        env.insert(
            "OML_SERVER__TRUSTED_PROXIES".to_owned(),
            "10.0.0.0/8, 192.0.2.1".to_owned(),
        );
        let cfg = Config::from_sources(Some(file), &|k| env.get(k).cloned())
            .expect("the environment's list wins");
        assert_eq!(
            cfg.trusted_proxies,
            TrustedProxies::parse(&["10.0.0.0/8", "192.0.2.1"]).expect("parses")
        );
        env.insert(
            "OML_SERVER__TRUSTED_PROXIES".to_owned(),
            "localhost".to_owned(),
        );
        let err = Config::from_sources(None, &|k| env.get(k).cloned())
            .expect_err("an entry that does not parse");
        assert!(err.to_string().contains("server.trusted_proxies"), "{err}");
    }

    #[test]
    fn metrics_listen_is_unset_by_default_and_a_value_equal_to_listen_or_no_address_stops_the_start()
     {
        let base = [
            ("OML_POSTGRES__URL", "postgres://serving@env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let cfg = Config::from_sources(None, &|k| env_of(&base).get(k).cloned())
            .expect("no metrics listener loads");
        assert_eq!(cfg.metrics_listen, None);
        let file = "[server]\nmetrics_listen = \"127.0.0.1:9464\"\n";
        let cfg = Config::from_sources(Some(file), &|k| env_of(&base).get(k).cloned())
            .expect("an address loads");
        assert_eq!(
            cfg.metrics_listen,
            Some("127.0.0.1:9464".parse().expect("an address"))
        );
        for bad in ["127.0.0.1:8080", "localhost:9464", "9464"] {
            let mut env = env_of(&base);
            env.insert("OML_SERVER__METRICS_LISTEN".to_owned(), bad.to_owned());
            let err = Config::from_sources(None, &|k| env.get(k).cloned()).expect_err("refused");
            assert!(
                err.to_string().contains("server.metrics_listen"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn brief_max_tokens_is_400_by_default_reads_oml_brief_max_tokens_and_refuses_zero_or_a_non_number()
     {
        let base = [
            ("OML_POSTGRES__URL", "postgres://serving@env/db"),
            ("OML_POSTGRES__OWNER_URL", "postgres://owner@env/db"),
        ];
        let cfg = Config::from_sources(None, &|k| env_of(&base).get(k).cloned())
            .expect("the default loads");
        assert_eq!(cfg.brief_max_tokens, 400);
        let file = "[brief]\nmax_tokens = 900\n";
        let cfg = Config::from_sources(Some(file), &|k| env_of(&base).get(k).cloned())
            .expect("a file value loads");
        assert_eq!(cfg.brief_max_tokens, 900);
        let mut env = env_of(&base);
        env.insert("OML_BRIEF__MAX_TOKENS".to_owned(), "120".to_owned());
        let cfg = Config::from_sources(Some(file), &|k| env.get(k).cloned())
            .expect("the environment wins");
        assert_eq!(cfg.brief_max_tokens, 120);
        for bad in ["0", "many", "-5"] {
            env.insert("OML_BRIEF__MAX_TOKENS".to_owned(), bad.to_owned());
            let err = Config::from_sources(None, &|k| env.get(k).cloned()).expect_err("refused");
            assert!(err.to_string().contains("brief.max_tokens"), "{bad}: {err}");
        }
    }

    /// Every line is one JSON object: `timestamp`, `level`, `target`, then
    /// the event's fields at the top level (ADR-0042 d1).
    #[test]
    fn the_lead_logs_tracing_subscribers_json_with_the_event_fields_at_the_top_level() {
        #[derive(Clone, Default)]
        struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("unpoisoned").extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf::default();
        let sink = buf.clone();
        tracing::subscriber::with_default(log_subscriber(move || sink.clone()), || {
            let span = tracing::info_span!("http_request", oml.request_id = "r1");
            let _entered = span.enter();
            tracing::info!(event = "connection_sweep", tenants = 3_u64);
        });
        let text = String::from_utf8(buf.0.lock().expect("unpoisoned").clone()).expect("UTF-8");
        let line: serde_json::Value = serde_json::from_str(text.trim()).expect("one JSON line");
        assert_eq!(line["event"], "connection_sweep");
        assert_eq!(line["tenants"], 3);
        assert_eq!(line["level"], "INFO");
        assert!(line["target"].is_string());
        let at = line["timestamp"].as_str().expect("a timestamp");
        assert!(at.ends_with('Z') && at.contains('T'), "RFC 3339 UTC: {at}");
        assert!(
            line.get("span").is_none() && line.get("spans").is_none(),
            "{line}"
        );
    }

    #[tokio::test]
    async fn keyring_creates_0600_and_reloads_the_same_key() {
        let dir = std::env::temp_dir().join(format!("oml-keyring-test-{}", std::process::id()));
        let path = dir.join("keyring");
        let _ = std::fs::remove_dir_all(&dir);
        let keyring = FileKeyring::new(path.clone());
        let first = keyring
            .load_or_create()
            .await
            .expect("mints on first start");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("keyring file exists")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "keyring file is 0600 (ADR-0023 d4)");
        }
        let second = keyring.load_or_create().await.expect("reloads");
        assert_eq!(first.to_bytes(), second.to_bytes());

        // Loosened permissions are refused, not warned about.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("chmod for the refusal case");
            let err = keyring.load_or_create().await.expect_err("0644 is refused");
            assert!(matches!(err, KeyringError::Permissions(_)), "got: {err:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
