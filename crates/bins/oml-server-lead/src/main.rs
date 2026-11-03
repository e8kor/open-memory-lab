//! `oml-server-lead` — boot order per deployment.md, ADR-0002 d7 and
//! ADR-0034 d4: configuration; the serving and owner pools; the role-split
//! checks before migrating, the migrations as the owner, the checks after;
//! the owner pool closed; then, over the serving pool alone, keyring, fork
//! check, the naming of rows that predate names, the enrollment token and
//! the listener. A role-split refusal exits non-zero, logged as
//! `postgres.role_split` with its reason, before the listener binds
//! (ADR-0034 d5). A failed fork check does not
//! exit: `/readyz` answers 503 and every write answers `system.unavailable`
//! until an operator intervenes (http-api.md "Health"). Nor does a failed
//! naming: a row the start could not name (ADR-0026 d11) keeps `/readyz` at
//! 503 and every request at `system.unavailable`.

use std::path::PathBuf;
use std::process::ExitCode;

use oml_app::ports::{KeyringStore as _, Signer as _};
use oml_pg::PgStore;
use oml_server_lead::{
    Config, ConfigError, FileKeyring, LeadSigner, SplitBootError, split_boot, start_lead,
};

/// Why the boot stopped: a `postgres.role_split` refusal, which `main`
/// logs under that code with its reason (ADR-0034 d5), or another fatal
/// failure.
enum BootError {
    RoleSplit(String),
    Fatal(String),
}

impl From<String> for BootError {
    fn from(e: String) -> Self {
        Self::Fatal(e)
    }
}

impl From<ConfigError> for BootError {
    fn from(e: ConfigError) -> Self {
        match e {
            ConfigError::RoleSplit(e) => Self::RoleSplit(e.reason.to_string()),
            ConfigError::Invalid(e) => Self::Fatal(e),
        }
    }
}

impl From<SplitBootError> for BootError {
    fn from(e: SplitBootError) -> Self {
        match e {
            SplitBootError::RoleSplit(check) => Self::RoleSplit(check.reason().map_or_else(
                // A catalog read that failed proves nothing and refuses as
                // well (ADR-0034 d5).
                || check.to_string(),
                |reason| reason.to_string(),
            )),
            other => Self::Fatal(other.to_string()),
        }
    }
}

fn config_path_arg() -> Result<Option<PathBuf>, String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "--config takes a path (configuration.md: the one flag besides scheduler modes)"
                        .to_owned()
                })?));
            }
            other => return Err(format!("unknown argument {other:?} (only --config <path>)")),
        }
    }
    Ok(path)
}

async fn run() -> Result<(), BootError> {
    let config_path = config_path_arg()?;
    let config = Config::load(config_path.as_deref(), &|name| std::env::var(name).ok())?;
    tracing::info!(listen = %config.listen, "oml-server-lead starting (standalone profile: always the lead of a cluster of one, ADR-0023 d16)");
    let build = oml_server_lead::build_info();
    tracing::info!(version = %build.version, commit = %build.commit, commit_time = %build.commit_time, build_time = %build.build_time, rustc = %build.rustc, target = %build.target, profile = %build.profile, "build");

    // ADR-0034 d4: the serving pool, then an owner pool that runs d5's
    // checks and the migrations and is closed before anything serves. The
    // store keeps the owner URL for a new tenant's partition DDL alone
    // (d11), which opens and closes its own owner connection.
    let store = PgStore::connect(&config.postgres_url)
        .await
        .map_err(|e| format!("postgres connect: {e}"))?
        .with_owner_url(&config.postgres_owner_url);
    let roles = split_boot(&store, &config.postgres_owner_url).await?;
    tracing::info!(owner_role = %roles.owner, serving_role = %roles.serving, "migrations applied as the owner; owner pool closed; serving as the serving role (ADR-0034 d4)");

    let keyring = FileKeyring::new(config.keyring_path.clone());
    let signing_key = keyring
        .load_or_create()
        .await
        .map_err(|e| format!("keyring: {e}"))?;
    let signer = LeadSigner::new(signing_key.clone());
    // The raw public key is logged alongside the fingerprint: it is public
    // material, and `oml audit verify-chain --signing-key` takes it as the
    // out-of-band operator input (chain-format.md key authentication).
    tracing::info!(
        fingerprint = %signer.fingerprint(),
        public_key = %signer.public_key_hex(),
        "deployment signing key loaded (kms-file)"
    );

    let start = start_lead(&store, signing_key).await?;
    match &start.fork_check {
        Ok(()) => tracing::info!(tenants = start.tenants.len(), "startup fork check passed"),
        // ADR-0002 d7: never sign on top of a fork. Writes are refused
        // (system.unavailable) and /readyz reports 503; reads keep flowing
        // so an operator can inspect.
        Err(failures) => tracing::error!(%failures, "STARTUP FORK CHECK FAILED — refusing writes"),
    }
    match &start.names {
        Ok(assigned) => tracing::info!(chains = assigned.len(), "startup names assigned"),
        // ADR-0026 d11: the lead names every row that predates names before
        // it serves; a row it could not name keeps it serving nothing,
        // rather than failing on each request that meets one.
        Err(failure) => {
            tracing::error!(%failure, "STARTUP NAMES CHECK FAILED — serving no request")
        }
    }
    if let Some(token) = &start.enrollment_token {
        // The one-time ceremony secret: stdout, once, never logged again
        // (deployment.md bootstrap — show-once).
        println!(
            "OML bootstrap: empty deployment — claim the first admin key with this single-use enrollment token:"
        );
        println!("{token}");
        println!(
            "POST /v2/bootstrap/claim {{\"enrollment_token\": \"...\", \"name\": \"<key-name>\"}}"
        );
    }

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .map_err(|e| format!("bind {}: {e}", config.listen))?;
    tracing::info!(addr = %config.listen, "serving both rings");
    axum::serve(listener, start.router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("serve: {e}"))?;
    tracing::info!("graceful shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    // Tracing to stderr: stdout is reserved for the show-once ceremony.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(BootError::RoleSplit(reason)) => {
            tracing::error!(code = "postgres.role_split", %reason, "postgres.role_split: refusing to serve before any listener binds (ADR-0034 d5)");
            ExitCode::FAILURE
        }
        Err(BootError::Fatal(e)) => {
            tracing::error!("fatal: {e}");
            ExitCode::FAILURE
        }
    }
}
