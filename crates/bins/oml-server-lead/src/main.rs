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
use std::sync::Arc;

use oml_app::ports::{KeyringStore as _, Signer as _};
use oml_fetch::PinnedFetcher;
use oml_pg::{PgStore, RoleSplitReason};
use oml_server_lead::otlp::Export;
use oml_server_lead::{
    Config, ConfigError, FileKeyring, LeadSigner, SplitBootError, split_boot, start_lead_with,
};
use oml_transport_http::traffic::Traffic;

/// The stable `event` of the line `main` logs when the role split refuses
/// the boot, beside `code` `postgres.role_split` and the d5 `reason`
/// (observability.md, structured logging; ADR-0034 d5).
const EVENT_ROLE_SPLIT_REFUSED: &str = "role_split_refused";

/// The stable `event` of the line that logs both role names once the owner
/// pool is closed (observability.md, structured logging; ADR-0034 d4).
const EVENT_ROLE_SPLIT_BOOTED: &str = "role_split_booted";

/// Why the boot stopped: a `postgres.role_split` refusal, which `main`
/// logs under that code (ADR-0034 d5), or another fatal failure.
enum BootError {
    RoleSplit(Refusal),
    Fatal(String),
}

/// A `postgres.role_split` refusal as `main` logs it: by one of d5's
/// reasons, whose name is the stable `reason` field, or as a catalog read
/// that did not complete, which proves nothing and refuses as well but
/// names no d5 reason, so its error goes in its own field (ADR-0034 d5).
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Reason(RoleSplitReason),
    CatalogUnread(String),
}

impl From<String> for BootError {
    fn from(e: String) -> Self {
        Self::Fatal(e)
    }
}

impl From<ConfigError> for BootError {
    fn from(e: ConfigError) -> Self {
        match e {
            ConfigError::RoleSplit(e) => Self::RoleSplit(Refusal::Reason(e.reason)),
            ConfigError::Invalid(e) => Self::Fatal(e),
        }
    }
}

impl From<SplitBootError> for BootError {
    fn from(e: SplitBootError) -> Self {
        match e {
            SplitBootError::RoleSplit(check) => Self::RoleSplit(check.reason().map_or_else(
                || Refusal::CatalogUnread(check.to_string()),
                Refusal::Reason,
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

async fn run(config: Result<Config, BootError>, export: Option<&Export>) -> Result<(), BootError> {
    let config = config?;
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
    tracing::info!(
        event = EVENT_ROLE_SPLIT_BOOTED,
        owner_role = roles.owner.as_str(),
        serving_role = roles.serving.as_str(),
        "migrations applied as the owner; owner pool closed; serving as the serving role (ADR-0034 d4)"
    );

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

    let traffic = Traffic::new(config.trusted_proxies.clone());
    let traffic = match export {
        Some(export) => traffic.with_tracer(export.request_tracer()),
        None => traffic,
    };
    // /metrics on its own listener, off unless server.metrics_listen names
    // an address; the API listener never routes it (ADR-0042 d11).
    let _metrics = match config.metrics_listen {
        None => None,
        Some(addr) => {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| format!("bind {addr} (server.metrics_listen): {e}"))?;
            tracing::info!(event = "metrics_listening", addr = %addr, "serving /metrics");
            let router =
                oml_transport_http::traffic::metrics::router(Arc::clone(traffic.metrics()));
            Some(tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    tracing::error!(event = "metrics_listener_failed", reason = %e, "the metrics listener stopped");
                }
            }))
        }
    };
    let start = start_lead_with(
        &store,
        signing_key,
        config.oauth_issuer.clone(),
        PinnedFetcher,
        traffic,
        config.web_pages(),
    )
    .await?;
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

    // The connections' operational removal, hourly from the start: nothing
    // else deletes an unused registration, a lapsed authorization or an
    // expired token (ADR-0031 d5, d9, d10).
    let _sweep = oml_server_lead::spawn_connection_sweep(store.clone());

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .map_err(|e| format!("bind {}: {e}", config.listen))?;
    tracing::info!(addr = %config.listen, "serving both rings");
    // The socket peer is the request line's client address, or the proxy
    // it reads `X-Forwarded-For` through (ADR-0042 d7).
    axum::serve(
        listener,
        start
            .router
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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
    // JSON lines on stderr, the whole process: stdout is reserved for the
    // show-once ceremony (observability.md, structured logging; ADR-0042
    // d1).
    // The configuration first, since it says whether spans are exported; a
    // configuration that does not load is logged once the subscriber is up.
    let config = config_path_arg().map_err(BootError::from).and_then(|path| {
        Config::load(path.as_deref(), &|name| std::env::var(name).ok()).map_err(BootError::from)
    });
    let export = match config.as_ref().ok().and_then(|c| c.otlp_endpoint.as_ref()) {
        None => Ok(None),
        Some(endpoint) => Export::start(endpoint).map(Some),
    };
    let tracer = export
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(Export::tracer);
    let _ = tracing::subscriber::set_global_default(oml_server_lead::exporting_subscriber(
        std::io::stderr,
        tracer,
    ));
    let outcome = match export {
        Ok(export) => {
            let outcome = run(config, export.as_ref()).await;
            if let Some(export) = export {
                // Flushes what is queued; it blocks, so off the runtime.
                let _ = tokio::task::spawn_blocking(move || export.shutdown()).await;
            }
            outcome
        }
        Err(e) => Err(BootError::Fatal(e)),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(BootError::RoleSplit(Refusal::Reason(reason))) => {
            tracing::error!(
                event = EVENT_ROLE_SPLIT_REFUSED,
                code = "postgres.role_split",
                reason = reason.as_str(),
                "postgres.role_split: refusing to serve before any listener binds (ADR-0034 d5)"
            );
            ExitCode::FAILURE
        }
        Err(BootError::RoleSplit(Refusal::CatalogUnread(error))) => {
            tracing::error!(event = EVENT_ROLE_SPLIT_REFUSED, code = "postgres.role_split", %error, "postgres.role_split: the check could not read the catalog; refusing to serve before any listener binds (ADR-0034 d5)");
            ExitCode::FAILURE
        }
        Err(BootError::Fatal(e)) => {
            tracing::error!("fatal: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BootError, Refusal};
    use oml_pg::{RoleSplitCheckError, RoleSplitError, RoleSplitReason};
    use oml_server_lead::{ConfigError, SplitBootError};

    fn refusal(e: BootError) -> Refusal {
        match e {
            BootError::RoleSplit(refusal) => refusal,
            BootError::Fatal(e) => panic!("a role-split failure is no fatal error: {e}"),
        }
    }

    #[test]
    fn a_role_split_refusal_logs_its_reason_as_the_stable_d5_name() {
        let refused = SplitBootError::RoleSplit(RoleSplitCheckError::Refused(RoleSplitError {
            reason: RoleSplitReason::SameRole,
        }));
        assert_eq!(
            refusal(refused.into()),
            Refusal::Reason(RoleSplitReason::SameRole)
        );
        let missing = ConfigError::RoleSplit(RoleSplitError {
            reason: RoleSplitReason::OwnerUrlMissing,
        });
        assert_eq!(
            refusal(missing.into()),
            Refusal::Reason(RoleSplitReason::OwnerUrlMissing)
        );
    }

    #[test]
    fn a_catalog_read_that_failed_refuses_with_no_reason_and_its_error_apart() {
        let unread =
            SplitBootError::RoleSplit(RoleSplitCheckError::Backend(sqlx::Error::PoolTimedOut));
        match refusal(unread.into()) {
            Refusal::CatalogUnread(error) => assert!(
                error.contains("could not read the catalog"),
                "the error says what failed: {error}"
            ),
            other @ Refusal::Reason(_) => {
                panic!("a catalog read that failed names no d5 reason; got {other:?}")
            }
        }
    }
}
