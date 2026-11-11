//! `oml-web --doctor` (ADR-0070 d4): the lines the lead's `oml doctor`
//! cannot print, because `oml-web`'s settings are this process's own. The
//! configuration's lines come from the same reading the start makes
//! ([`Config::from_lookup`]), so the doctor and the start never disagree;
//! `main` adds the API's readiness after them.

use std::fmt;

use crate::config::{
    CLIENT_SECRET_MIN, Config, ConfigError, KEY_API_URL, KEY_CLIENT_SECRET, env_name,
};

/// A doctor line's level, as `oml doctor` prints them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        })
    }
}

/// The configuration's lines, read through `lookup` (an environment
/// variable name to its value): `web.api_url` as `ok`, `warn` (plain http
/// to a non-loopback host, allowed by `web.allow_plain_http_api`) or `fail`
/// (missing or refused), then `fail` for any other setting the start
/// refuses, naming it. No line carries the client secret.
pub fn config_lines(lookup: impl Fn(&str) -> Option<String>) -> Vec<(Level, String)> {
    let mut lines = Vec::new();
    // web.api_url alone: every other setting held valid, so its line says
    // what the start would say of it whatever else is wrong.
    let placeholder = "s".repeat(CLIENT_SECRET_MIN);
    let secret_name = env_name(KEY_CLIENT_SECRET);
    let api_only = Config::from_lookup(|name| {
        if name == secret_name {
            Some(placeholder.clone())
        } else if name == env_name(KEY_API_URL)
            || name == env_name(crate::config::KEY_ALLOW_PLAIN_HTTP)
        {
            lookup(name)
        } else {
            None
        }
    });
    lines.push(match api_only {
        Ok(config) if config.allow_plain_http_api => (
            Level::Warn,
            format!(
                "{KEY_API_URL}: {} is plain http to a non-loopback host, allowed by web.allow_plain_http_api; session credentials cross it in clear",
                config.api_url
            ),
        ),
        Ok(config) => (Level::Ok, format!("{KEY_API_URL}: {}", config.api_url)),
        Err(err) => (Level::Fail, err.to_string()),
    });
    match Config::from_lookup(&lookup) {
        Ok(config) => lines.push((
            Level::Ok,
            format!(
                "web.listen: {}; {KEY_CLIENT_SECRET} set; web.public_url: {}",
                config.listen,
                config
                    .public_url
                    .as_deref()
                    .unwrap_or("read from the API's issuer at start")
            ),
        )),
        Err(ConfigError::Missing(key) | ConfigError::Invalid(key, _)) if key == KEY_API_URL => {}
        Err(err) => lines.push((Level::Fail, err.to_string())),
    }
    lines
}
