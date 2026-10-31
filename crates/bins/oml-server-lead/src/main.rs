//! `oml-server-lead` — boot order per deployment.md and ADR-0002 d7:
//! configuration, Postgres, migrations, keyring, fork check, names check,
//! then the listener. A failed fork check does not exit: `/readyz` answers
//! 503 and every write answers `system.unavailable` until an operator
//! intervenes (http-api.md "Health"). Nor does a failed names check: rows
//! that predate names, which this build does not name yet (ADR-0026 d11),
//! keep `/readyz` at 503 and every request at `system.unavailable`.

use std::path::PathBuf;
use std::process::ExitCode;

use oml_app::ports::{KeyringStore as _, Signer as _};
use oml_pg::PgStore;
use oml_server_lead::{
    Config, FileKeyring, LeadSigner, enumerate_tenants, lead_state, mint_enrollment_token, pg_ping,
    startup_fork_check, startup_names_check,
};

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

async fn run() -> Result<(), String> {
    let config_path = config_path_arg()?;
    let config = Config::load(config_path.as_deref(), &|name| std::env::var(name).ok())?;
    tracing::info!(listen = %config.listen, "oml-server-lead starting (standalone profile: always the lead of a cluster of one, ADR-0023 d16)");
    let build = oml_server_lead::build_info();
    tracing::info!(version = %build.version, commit = %build.commit, commit_time = %build.commit_time, build_time = %build.build_time, rustc = %build.rustc, target = %build.target, profile = %build.profile, "build");

    let store = PgStore::connect(&config.postgres_url)
        .await
        .map_err(|e| format!("postgres connect: {e}"))?;
    store
        .migrate()
        .await
        .map_err(|e| format!("migrate on boot: {e}"))?;
    tracing::info!("migrations applied");

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

    let tenants = enumerate_tenants(store.pool())
        .await
        .map_err(|e| format!("tenant enumeration: {e}"))?;
    let fork_check_ok = match startup_fork_check(&store, signer.verifying_key(), &tenants).await {
        Ok(()) => {
            tracing::info!(tenants = tenants.len(), "startup fork check passed");
            true
        }
        Err(failures) => {
            // ADR-0002 d7: never sign on top of a fork. Writes are refused
            // (system.unavailable) and /readyz reports 503; reads keep
            // flowing so an operator can inspect.
            tracing::error!(%failures, "STARTUP FORK CHECK FAILED — refusing writes");
            false
        }
    };
    let names_assigned = match startup_names_check(&store, &tenants).await {
        Ok(()) => true,
        Err(failure) => {
            // ADR-0026 d11: the lead names every row that predates names
            // before it serves; this build does not yet, so it serves
            // nothing rather than fail on each request that meets one.
            tracing::error!(%failure, "STARTUP NAMES CHECK FAILED — serving no request");
            false
        }
    };

    let enrollment_token = if tenants.is_empty() && fork_check_ok {
        let token = mint_enrollment_token();
        // The one-time ceremony secret: stdout, once, never logged again
        // (deployment.md bootstrap — show-once).
        println!(
            "OML bootstrap: empty deployment — claim the first admin key with this single-use enrollment token:"
        );
        println!("{token}");
        println!(
            "POST /v2/bootstrap/claim {{\"enrollment_token\": \"...\", \"name\": \"<key-name>\"}}"
        );
        Some(token)
    } else {
        None
    };

    let db_ping = pg_ping(store.pool().clone());
    let router = lead_state(
        &store,
        signing_key,
        db_ping,
        fork_check_ok,
        enrollment_token,
    )
    .with_names_assigned(names_assigned)
    .into_router();

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .map_err(|e| format!("bind {}: {e}", config.listen))?;
    tracing::info!(addr = %config.listen, "serving both rings");
    axum::serve(listener, router)
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
        Err(e) => {
            tracing::error!("fatal: {e}");
            ExitCode::FAILURE
        }
    }
}
