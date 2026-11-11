//! `oml-web`: the web console's process (ADR-0070 d4 as amended on
//! 2026-11-10). Reads its configuration from the environment, refuses to
//! start without `web.api_url` and `web.client_secret`, reads the public
//! origin from the API unless `web.public_url` names it, and serves the
//! pages on `web.listen`. `oml-web --doctor` prints the doctor lines instead
//! and exits `1` when any is `fail` (ADR-0070 d4).

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use oml_web::config::{API_TIMEOUT, KEY_PUBLIC_URL};
use oml_web::{Config, Layers, Web, api, router};

/// How long the start waits for the API's discovery document, a second
/// apart, before it refuses to start: the API may start after this process.
const DISCOVERY_ATTEMPTS: u32 = 30;

/// `--doctor`: the configuration's lines, then the API's readiness when the
/// configuration holds, each `<level> <setting>: <detail>`.
async fn doctor() -> ExitCode {
    use oml_web::doctor::{Level, config_lines};
    let mut lines = config_lines(|name| std::env::var(name).ok());
    if let Ok(config) = Config::from_env() {
        let client = oml_sdk::HttpTransport::new(&config.api_url, API_TIMEOUT)
            .map_err(|e| e.to_string())
            .and_then(|t| api(Arc::new(t), config.client_secret.expose()));
        let line = match &client {
            Err(e) => (Level::Fail, format!("api: {e}")),
            Ok(api) => match api.readyz().await {
                Ok(ready)
                    if ready.status == "ok" && ready.web_client.as_deref() == Some("none") =>
                {
                    (
                        Level::Fail,
                        format!(
                            "api: {} is ready but accepts no web client identity; set web.client_secret on the lead to oml-web's",
                            config.api_url
                        ),
                    )
                }
                Ok(ready) if ready.status == "ok" && ready.web_client.is_none() => (
                    Level::Fail,
                    format!(
                        "api: {} is ready but names no web client identity; it is a lead from before ADR-0070, which accepts no capacity credential",
                        config.api_url
                    ),
                ),
                Ok(ready) if ready.status == "ok" => (
                    Level::Ok,
                    format!(
                        "api: {} is ready (web client identity: {})",
                        config.api_url,
                        ready.web_client.as_deref().unwrap_or_default()
                    ),
                ),
                Ok(ready) => (
                    Level::Fail,
                    format!("api: {} answered {}", config.api_url, ready.status),
                ),
                Err(e) => (
                    Level::Fail,
                    format!("api: {} is unreachable or not ready: {e}", config.api_url),
                ),
            },
        };
        let ready = line.0 == Level::Ok;
        lines.push(line);
        // Whether the lead takes this secret, once it is ready (#230).
        if let (true, Ok(api)) = (ready, &client) {
            lines.push(oml_web::doctor::secret_line(api, &config.api_url).await);
        }
    }
    let failed = lines.iter().any(|(level, _)| *level == Level::Fail);
    for (level, text) in lines {
        println!("{level} {text}");
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("--doctor") => return doctor().await,
        Some("--version") => {
            println!("oml-web {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("oml-web: unknown argument {other}; oml-web takes --doctor or --version");
            return ExitCode::from(2);
        }
    }
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
