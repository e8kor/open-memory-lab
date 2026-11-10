//! The process's configuration: the API it is a client of, where it listens
//! and the credential that names it to the API (ADR-0070 d2, d4, as amended
//! on 2026-11-10: `oml-web` runs only as its own process).
//!
//! Each key is read from the environment under the name the lead's keys use,
//! `OML_` and the key upper-cased with `.` as `__`, so `web.api_url` is
//! `OML_WEB__API_URL`.

use std::fmt;
use std::net::SocketAddr;

/// The API's base URL, `scheme://host[:port]` with no path. Required: there
/// is no in-process API to fall back on.
pub const KEY_API_URL: &str = "web.api_url";
/// The address the pages listen on.
pub const KEY_LISTEN: &str = "web.listen";
/// The web client secret the API checks on every `omls_` credential
/// (ADR-0070 d2). Read, never logged.
pub const KEY_CLIENT_SECRET: &str = "web.client_secret";
/// Permits a plain `http://` `web.api_url` to a host that is not loopback,
/// for a private network (ADR-0070 d4). Off unless set to `true`.
pub const KEY_ALLOW_PLAIN_HTTP: &str = "web.allow_plain_http_api";

/// Where the pages listen when `web.listen` is unset: loopback, beside the
/// lead's `127.0.0.1:8090`, for the reverse proxy in front of both.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8091";

/// The environment variable that carries `key`.
#[must_use]
pub fn env_name(key: &str) -> String {
    format!("OML_{}", key.to_uppercase().replace('.', "__"))
}

/// Why the process refuses to start, naming the key at fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A required key is unset or empty.
    Missing(&'static str),
    /// A key is set to a value the rule refuses, and why.
    Invalid(&'static str, &'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(key) => write!(f, "{key} ({}) is required", env_name(key)),
            Self::Invalid(key, why) => write!(f, "{key} ({}): {why}", env_name(key)),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The web client secret. Its `Debug` prints no value, so no log line or
/// panic message can carry it.
#[derive(Clone, PartialEq, Eq)]
pub struct ClientSecret(String);

impl ClientSecret {
    /// The value, for the header the API checks; nothing else reads it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClientSecret(redacted)")
    }
}

/// The loaded configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub api_url: String,
    pub listen: SocketAddr,
    pub client_secret: ClientSecret,
    /// `web.allow_plain_http_api`: the `http://` hop to a non-loopback host
    /// is allowed, and the process logs a warning at every start.
    pub allow_plain_http_api: bool,
}

/// Whether `authority` (`host[:port]`, no userinfo) names a loopback host.
fn is_loopback_host(authority: &str) -> bool {
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// The shortest secret accepted: 32 characters, so a placeholder such as
/// `changeme` is refused at start rather than presented to the API.
pub const CLIENT_SECRET_MIN: usize = 32;

impl Config {
    /// The configuration from the process environment.
    ///
    /// # Errors
    /// The first key that is missing or invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The configuration from `lookup`, which maps an environment variable
    /// name to its value.
    ///
    /// # Errors
    /// The first key that is missing or invalid.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |key: &'static str| lookup(&env_name(key)).filter(|v| !v.trim().is_empty());
        let api_url = get(KEY_API_URL).ok_or(ConfigError::Missing(KEY_API_URL))?;
        let api_url = api_url.trim().trim_end_matches('/').to_owned();
        let plain = api_url.starts_with("http://");
        let rest = api_url
            .strip_prefix("https://")
            .or_else(|| api_url.strip_prefix("http://"))
            .ok_or(ConfigError::Invalid(
                KEY_API_URL,
                "must start with http:// or https://",
            ))?;
        if rest.is_empty() || rest.contains('/') {
            return Err(ConfigError::Invalid(
                KEY_API_URL,
                "must name a host and no path",
            ));
        }
        if rest.contains(['@', '?', '#']) {
            return Err(ConfigError::Invalid(
                KEY_API_URL,
                "must carry no userinfo, query or fragment",
            ));
        }
        let allow_plain_http_api =
            get(KEY_ALLOW_PLAIN_HTTP).is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
        if plain && !is_loopback_host(rest) && !allow_plain_http_api {
            return Err(ConfigError::Invalid(
                KEY_API_URL,
                "plain http to a non-loopback host sends session credentials in clear; use https://, or set web.allow_plain_http_api = true for a private network",
            ));
        }
        let listen = get(KEY_LISTEN)
            .unwrap_or_else(|| DEFAULT_LISTEN.to_owned())
            .trim()
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::Invalid(KEY_LISTEN, "must be an address and port"))?;
        let secret = get(KEY_CLIENT_SECRET).ok_or(ConfigError::Missing(KEY_CLIENT_SECRET))?;
        if secret.chars().count() < CLIENT_SECRET_MIN {
            return Err(ConfigError::Invalid(
                KEY_CLIENT_SECRET,
                "must be at least 32 characters",
            ));
        }
        let warn_plain = plain && allow_plain_http_api && !is_loopback_host(rest);
        Ok(Self {
            api_url,
            listen,
            client_secret: ClientSecret(secret),
            allow_plain_http_api: warn_plain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|name| env.get(name).cloned())
    }

    #[test]
    fn the_keys_are_read_under_the_leads_mechanical_env_names() {
        assert_eq!(env_name(KEY_API_URL), "OML_WEB__API_URL");
        assert_eq!(env_name(KEY_LISTEN), "OML_WEB__LISTEN");
        assert_eq!(env_name(KEY_CLIENT_SECRET), "OML_WEB__CLIENT_SECRET");
    }

    #[test]
    fn without_web_api_url_the_process_refuses_to_start_naming_the_key() {
        let err = load(&[("OML_WEB__CLIENT_SECRET", SECRET)]).unwrap_err();
        assert_eq!(err, ConfigError::Missing(KEY_API_URL));
        assert!(err.to_string().contains("web.api_url"));
        assert!(err.to_string().contains("OML_WEB__API_URL"));
    }

    #[test]
    fn without_the_client_secret_the_process_refuses_to_start() {
        let err = load(&[("OML_WEB__API_URL", "http://127.0.0.1:8090")]).unwrap_err();
        assert_eq!(err, ConfigError::Missing(KEY_CLIENT_SECRET));
    }

    #[test]
    fn a_short_client_secret_is_refused() {
        let err = load(&[
            ("OML_WEB__API_URL", "http://127.0.0.1:8090"),
            ("OML_WEB__CLIENT_SECRET", "changeme"),
        ])
        .unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(KEY_CLIENT_SECRET, _)));
    }

    #[test]
    fn an_api_url_with_a_path_or_no_scheme_is_refused() {
        for bad in ["127.0.0.1:8090", "http://", "https://api.example/v2"] {
            let err = load(&[
                ("OML_WEB__API_URL", bad),
                ("OML_WEB__CLIENT_SECRET", SECRET),
            ])
            .unwrap_err();
            assert!(matches!(err, ConfigError::Invalid(KEY_API_URL, _)), "{bad}");
        }
    }

    #[test]
    fn the_listen_address_defaults_to_loopback_8091_and_a_trailing_slash_is_dropped() {
        let config = load(&[
            ("OML_WEB__API_URL", "https://memory.example/"),
            ("OML_WEB__CLIENT_SECRET", SECRET),
        ])
        .unwrap();
        assert_eq!(config.api_url, "https://memory.example");
        assert_eq!(config.listen, DEFAULT_LISTEN.parse().unwrap());
    }

    #[test]
    fn a_plain_http_api_url_to_a_non_loopback_host_is_refused_unless_allowed() {
        let base = [("OML_WEB__CLIENT_SECRET", SECRET)];
        let with = |url: &'static str, allow: Option<&'static str>| {
            let mut v = base.to_vec();
            v.push(("OML_WEB__API_URL", url));
            if let Some(a) = allow {
                v.push(("OML_WEB__ALLOW_PLAIN_HTTP_API", a));
            }
            load(&v)
        };
        for ok in [
            "http://127.0.0.1:8090",
            "http://localhost:8090",
            "http://[::1]:8090",
            "https://oml:8090",
        ] {
            assert!(with(ok, None).is_ok(), "{ok}");
        }
        let err = with("http://oml:8090", None).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(KEY_API_URL, _)));
        assert!(err.to_string().contains("allow_plain_http_api"));
        assert!(with("http://oml:8090", Some("false")).is_err());
        let config = with("http://oml:8090", Some("true")).unwrap();
        assert!(config.allow_plain_http_api);
        assert!(
            !with("https://oml:8090", Some("true"))
                .unwrap()
                .allow_plain_http_api
        );
    }

    #[test]
    fn an_api_url_with_userinfo_query_or_fragment_is_refused() {
        for bad in ["https://u:p@h", "http://127.0.0.1?x=1", "https://h#f"] {
            let err = load(&[
                ("OML_WEB__API_URL", bad),
                ("OML_WEB__CLIENT_SECRET", SECRET),
            ])
            .unwrap_err();
            assert!(matches!(err, ConfigError::Invalid(KEY_API_URL, _)), "{bad}");
        }
    }

    #[test]
    fn the_client_secret_never_appears_in_debug_output() {
        let config = load(&[
            ("OML_WEB__API_URL", "http://127.0.0.1:8090"),
            ("OML_WEB__CLIENT_SECRET", SECRET),
        ])
        .unwrap();
        assert!(!format!("{config:?}").contains(SECRET));
        assert_eq!(config.client_secret.expose(), SECRET);
    }
}
