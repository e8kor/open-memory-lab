//! `oml-web`: the web console's process (ADR-0070 d4 as amended on
//! 2026-11-10). Reads its configuration from the environment, refuses to
//! start without `web.api_url` and `web.client_secret`, and serves the pages
//! on `web.listen`.

use std::process::ExitCode;
use std::sync::Arc;

use oml_web::{Config, Layers, router};

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("oml-web: refusing to start: {err}");
            return ExitCode::from(2);
        }
    };
    if config.allow_plain_http_api {
        eprintln!(
            "oml-web: warning: web.api_url {} is plain http to a non-loopback host; session credentials cross it in clear (web.allow_plain_http_api is set)",
            config.api_url
        );
    }
    let layers = Layers {
        public_origin: None,
        log: Arc::new(|line| println!("{line}")),
    };
    let listener = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("oml-web: cannot listen on {}: {err}", config.listen);
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "oml-web {} listening on {}, API at {}",
        env!("CARGO_PKG_VERSION"),
        config.listen,
        config.api_url
    );
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match axum::serve(listener, router(layers))
        .with_graceful_shutdown(shutdown)
        .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("oml-web: {err}");
            ExitCode::from(1)
        }
    }
}
