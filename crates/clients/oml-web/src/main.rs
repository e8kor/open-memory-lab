//! `oml-web`: the web console's process (ADR-0070 d4 as amended on
//! 2026-11-10). Reads its configuration from the environment, refuses to
//! start without `web.api_url` and `web.client_secret`, reads the public
//! origin from the API unless `web.public_url` names it, and serves the
//! pages on `web.listen`.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use oml_web::config::{API_TIMEOUT, KEY_PUBLIC_URL};
use oml_web::{Config, Layers, Web, api, router};

/// How long the start waits for the API's discovery document, a second
/// apart, before it refuses to start: the API may start after this process.
const DISCOVERY_ATTEMPTS: u32 = 30;

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
    let transport = match oml_sdk::HttpTransport::new(&config.api_url, API_TIMEOUT) {
        Ok(transport) => transport,
        Err(err) => {
            eprintln!("oml-web: refusing to start: {err}");
            return ExitCode::from(2);
        }
    };
    let api = match api(Arc::new(transport), config.client_secret.expose()) {
        Ok(api) => api,
        Err(err) => {
            eprintln!("oml-web: refusing to start: {err}");
            return ExitCode::from(2);
        }
    };
    let public_origin = match config.public_url.clone() {
        Some(url) => url,
        None => {
            let mut found = None;
            for _ in 0..DISCOVERY_ATTEMPTS {
                match api.issuer().await {
                    Ok(issuer) => {
                        // The lead's Issuer has no trailing slash (it refuses one), so
                        // this is the spelling its own same-origin check compares.
                        found = Some(issuer);
                        break;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
                }
            }
            let Some(issuer) = found else {
                eprintln!(
                    "oml-web: refusing to start: {KEY_PUBLIC_URL} is unset and {} answered no issuer at /.well-known/oauth-authorization-server; set {KEY_PUBLIC_URL} or set oauth.issuer on the lead",
                    config.api_url
                );
                return ExitCode::from(1);
            };
            issuer
        }
    };
    let web = Web {
        layers: Layers {
            public_origin: Some(public_origin.clone()),
            log: Arc::new(|line| println!("{line}")),
        },
        api,
    };
    let listener = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("oml-web: cannot listen on {}: {err}", config.listen);
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "oml-web {} listening on {}, API at {}, public origin {public_origin}",
        env!("CARGO_PKG_VERSION"),
        config.listen,
        config.api_url
    );
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match axum::serve(listener, router(web))
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
