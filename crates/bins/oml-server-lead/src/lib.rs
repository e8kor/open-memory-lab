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
use oml_app::domain::{EventTime, KeySet};
use oml_app::ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use oml_app::oauth_fetch::CappedFetcher;
use oml_app::ports::{
    AdmissionEnvelope, Clock, DocumentFetcher, IdGen, KeyringError, KeyringStore, ScanOutcome,
    Scanner, Signer,
};
use oml_app::types::AssignedNames;
use oml_fetch::PinnedFetcher;
use oml_pg::{
    ForkCheckError, PgStore, RoleSplitCheckError, RoleSplitError, RoleSplitReason,
    role_split_postflight, role_split_preflight,
};
use oml_transport_http::traffic::{ScopeObserver, Traffic, TrustedProxies};
use oml_transport_http::{ApiState, BuildInfo, DbPing, time::system_event_time};

use crate::timed::Timed;

// ---------------------------------------------------------------------------
// Configuration: defaults < file < environment (configuration.md)
// ---------------------------------------------------------------------------

/// The boot-configuration slice this composition reads (configuration.md
/// per-binary table: `postgres.*`, `kms.*`, `server.listen`, `oauth.issuer`;
/// `server.role` is not accepted in standalone — a standalone node is always
/// the lead).
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// `brief.max_tokens`: the budget a workarea's brief is cut to, in
    /// budget tokens; 400 when unset, unmeasured (ADR-0046 d8; FR-77).
    pub brief_max_tokens: u64,
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
const KEY_BRIEF_MAX_TOKENS: &str = "brief.max_tokens";

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
        Ok(Self {
            postgres_url,
            postgres_owner_url,
            listen,
            keyring_path,
            oauth_issuer,
            trusted_proxies,
            metrics_listen,
            otlp_endpoint,
            brief_max_tokens,
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
    store.migrate_as(owner).await?;
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

/// The PoC deterministic scanner: rules, not a model — same input, same
/// verdict (admission.md). The rule set is two literal markers.
pub struct MarkerScanner;

impl Scanner for MarkerScanner {
    fn scan(&self, envelope: &AdmissionEnvelope<'_>) -> ScanOutcome {
        if envelope.body.contains("[oml:veto]") {
            ScanOutcome::Veto("poc-rules/marker-veto".to_owned())
        } else if envelope.body.contains("[oml:hold]") {
            ScanOutcome::Hold("poc-rules/marker-hold".to_owned())
        } else {
            ScanOutcome::Pass
        }
    }
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
pub type LeadApp = App<LeadStore, LeadSigner, SystemClock, OsIds, MarkerScanner>;
pub type LeadState = ApiState<LeadStore, LeadSigner, SystemClock, OsIds, MarkerScanner>;

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
    App::new(
        Timed::new(store.clone()),
        LeadSigner::new(signing_key),
        SystemClock,
        OsIds,
        MarkerScanner,
    )
    .with_observer(Arc::new(ScopeObserver))
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
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
    )
    .await
}

/// [`start_lead`] with `fetcher` as the outbound adapter under the fetch
/// caps in place of [`PinnedFetcher`], so a test sees what the composed
/// page asks of the network without reaching it, and the request line
/// configured by `traffic` (ADR-0042 d7), as `main` starts it.
pub async fn start_lead_with<F: DocumentFetcher + 'static>(
    store: &PgStore,
    signing_key: SigningKey,
    oauth_issuer: Option<Issuer>,
    fetcher: F,
    traffic: Traffic,
    brief_max_tokens: u64,
) -> Result<LeadStart, String> {
    let tenants = enumerate_tenants(store.pool())
        .await
        .map_err(|e| format!("tenant enumeration: {e}"))?;
    let fork_check = startup_fork_check(store, signing_key.verifying_key(), &tenants).await;
    let app = lead_app(store, signing_key).with_brief_max_tokens(brief_max_tokens);
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
    let router = composed(
        app,
        store,
        pg_ping(store.pool().clone()),
        fork_check.is_ok(),
        enrollment_token.clone(),
        fetcher,
        traffic,
    )
    .with_names_assigned(names.is_ok())
    .into_router();
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
