//! The lead composition pieces (workspace-plan.md: binaries are thin
//! compositions): configuration (oml.toml + `OML_` environment,
//! configuration.md), the kms-file keyring (ADR-0023 d4), the runtime
//! clock/id/scanner adapters, the startup fork check (ADR-0002 d7) and the
//! state constructor the integration tests share with `main`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::Row;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use oml_app::App;
use oml_app::domain::{EventTime, KeySet};
use oml_app::ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use oml_app::ports::{
    AdmissionEnvelope, Clock, IdGen, KeyringError, KeyringStore, ScanOutcome, Scanner, Signer,
};
use oml_pg::{ForkCheckError, PgStore};
use oml_transport_http::{ApiState, BuildInfo, DbPing, time::system_event_time};

// ---------------------------------------------------------------------------
// Configuration: defaults < file < environment (configuration.md)
// ---------------------------------------------------------------------------

/// The boot-configuration slice this composition reads (configuration.md
/// per-binary table: `postgres.*`, `kms.*`, `server.listen`; `server.role` is
/// not accepted in standalone — a standalone node is always the lead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub postgres_url: String,
    pub listen: String,
    pub keyring_path: PathBuf,
}

const KEY_POSTGRES_URL: &str = "postgres.url";
const KEY_LISTEN: &str = "server.listen";
const KEY_KMS_ADAPTER: &str = "kms.adapter";
const KEY_KEYRING_PATH: &str = "kms.keyring_path";

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
    pub fn from_sources(
        file: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let doc: Option<toml::Value> = match file {
            None => None,
            Some(text) => Some(
                text.parse::<toml::Value>()
                    .map_err(|e| format!("the configuration file is not TOML: {e}"))?,
            ),
        };
        let lookup = |key: &str| -> Option<String> {
            if let Some(v) = env(&env_name(key)) {
                return Some(v);
            }
            doc.as_ref()
                .and_then(|d| file_lookup(d, key))
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
        };
        // The PoC composes the kms-file adapter only (ADR-0023 d4); naming
        // another adapter is refused rather than silently ignored.
        if let Some(adapter) = lookup(KEY_KMS_ADAPTER)
            && adapter != "file"
        {
            return Err(format!(
                "kms.adapter {adapter:?} is not available in this composition (kms-file only, ADR-0023 d4)"
            ));
        }
        let postgres_url = lookup(KEY_POSTGRES_URL).ok_or_else(|| {
            format!("{KEY_POSTGRES_URL} is required (none — required, configuration.md)")
        })?;
        let listen = lookup(KEY_LISTEN).unwrap_or_else(|| "127.0.0.1:8080".to_owned());
        let keyring_path =
            lookup(KEY_KEYRING_PATH).map_or_else(|| PathBuf::from("oml-keyring"), PathBuf::from);
        Ok(Self {
            postgres_url,
            listen,
            keyring_path,
        })
    }

    /// Reads `--config <path>` (when given) and the process environment.
    pub fn load(
        config_path: Option<&Path>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let file = match config_path {
            None => None,
            Some(path) => Some(
                std::fs::read_to_string(path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?,
            ),
        };
        Self::from_sources(file.as_deref(), env)
    }
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

/// The single-use enrollment token minted against an empty database
/// (deployment.md bootstrap; the PoC lead prints it to stdout).
#[must_use]
pub fn mint_enrollment_token() -> String {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes).expect("the OS random source is available");
    format!("enr_{}", hex_encode(&bytes))
}

// ---------------------------------------------------------------------------
// The composed state
// ---------------------------------------------------------------------------

pub type LeadApp = App<PgStore, LeadSigner, SystemClock, OsIds, MarkerScanner>;
pub type LeadState = ApiState<PgStore, LeadSigner, SystemClock, OsIds, MarkerScanner>;

/// A `/readyz` probe over the composition's own pool.
#[must_use]
pub fn pg_ping(pool: PgPool) -> DbPing {
    Arc::new(move || {
        let pool = pool.clone();
        Box::pin(async move { sqlx::query("SELECT 1").execute(&pool).await.is_ok() })
    })
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
    let app = App::new(
        store.clone(),
        LeadSigner::new(signing_key),
        SystemClock,
        OsIds,
        MarkerScanner,
    );
    ApiState::new(
        Arc::new(app),
        store.clone(),
        db_ping,
        fork_check_ok,
        enrollment_token,
    )
    .with_build(build_info())
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
        let file =
            "[postgres]\nurl = \"postgres://file/db\"\n[server]\nlisten = \"0.0.0.0:9999\"\n";
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
            ("OML_KMS__ADAPTER", "aws"),
        ]);
        let err = Config::from_sources(None, &|k| env.get(k).cloned())
            .expect_err("a non-file kms adapter is refused in this composition");
        assert!(err.contains("kms.adapter"), "got: {err}");
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
