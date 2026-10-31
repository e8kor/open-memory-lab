//! The issuer, the two discovery documents and the `/mcp` challenge, all
//! built from `oauth.issuer` alone, never from a request's `Host`
//! (ADR-0031 d1, d2, d3; `docs/api/http-api.md`, "Connections: OAuth for
//! hosted MCP clients"; `docs/operations/configuration.md`, `oauth.issuer`).

use serde_json::{Value, json};

use super::refusal::Refusal;
use super::syntax;

/// The one scope; it means "act as the connected key at `/mcp`" and
/// narrows nothing (ADR-0031 d4).
pub const SCOPE: &str = "mcp";

/// The protected resource's `resource_name` (`docs/api/openapi.yaml`).
pub const RESOURCE_NAME: &str = "Open Memory Lab";

/// `oauth.issuer`: the public origin — `https`, a lower-case ASCII DNS
/// name, a port only if not 443, no path and no trailing slash. The one
/// protected resource is `<issuer>/mcp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issuer {
    origin: String,
    resource: String,
}

/// Why a configured `oauth.issuer` is not a public origin. Any of these
/// stops the process at start (`docs/operations/configuration.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IssuerError {
    #[error("oauth.issuer must begin https:// (configuration.md, oauth.issuer)")]
    NotHttps,
    #[error(
        "oauth.issuer's host must be a lower-case ASCII DNS name, with no userinfo (configuration.md, oauth.issuer)"
    )]
    Host,
    #[error(
        "oauth.issuer's port must be 1 to 65535, written with no leading zero (configuration.md, oauth.issuer)"
    )]
    Port,
    #[error(
        "oauth.issuer names port 443, which an https origin omits (configuration.md, oauth.issuer)"
    )]
    DefaultPort,
    #[error("oauth.issuer ends in a slash; an origin has none (configuration.md, oauth.issuer)")]
    TrailingSlash,
    #[error(
        "oauth.issuer has a path, query or fragment; it is an origin (configuration.md, oauth.issuer)"
    )]
    Path,
}

impl Issuer {
    /// Reads `oauth.issuer`, refusing any value that is not exactly a
    /// public origin, so the documents, the challenge and every
    /// connection's `resource` are built from one spelling.
    pub fn parse(value: &str) -> Result<Self, IssuerError> {
        let after_scheme = value
            .strip_prefix("https://")
            .ok_or(IssuerError::NotHttps)?;
        let authority_len = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        let (authority, rest) = after_scheme.split_at(authority_len);
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if !syntax::dns_host(host) {
            return Err(IssuerError::Host);
        }
        match port {
            Some("443") => return Err(IssuerError::DefaultPort),
            Some(port) if syntax::port(port).is_none() => return Err(IssuerError::Port),
            _ => {}
        }
        match rest {
            "" => Ok(Issuer {
                origin: value.to_owned(),
                resource: format!("{value}/mcp"),
            }),
            "/" => Err(IssuerError::TrailingSlash),
            _ => Err(IssuerError::Path),
        }
    }

    /// The origin, which is the RFC 8414 `issuer` and the `iss` of every
    /// authorization response.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.origin
    }

    /// `<issuer>/mcp`, the one protected resource.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// The protected-resource document's URL, path-inserted (RFC 9728
    /// §3.1): `<issuer>/.well-known/oauth-protected-resource/mcp`.
    #[must_use]
    pub fn protected_resource_metadata_url(&self) -> String {
        format!("{}/.well-known/oauth-protected-resource/mcp", self.origin)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
}

/// `GET /.well-known/oauth-protected-resource/mcp` (RFC 9728): `resource`
/// exactly `<issuer>/mcp`, one authorization server — Claude uses the first
/// and never falls back — one scope, the header method only.
#[must_use]
pub fn protected_resource_document(issuer: &Issuer) -> Value {
    json!({
        "resource": issuer.resource(),
        "authorization_servers": [issuer.as_str()],
        "scopes_supported": [SCOPE],
        "bearer_methods_supported": ["header"],
        "resource_name": RESOURCE_NAME,
    })
}

/// `GET /.well-known/oauth-authorization-server` (RFC 8414): the code flow
/// with S256 PKCE for public clients, client ID metadata documents, and
/// `iss` on every authorization response (RFC 9207 §3).
#[must_use]
pub fn authorization_server_document(issuer: &Issuer) -> Value {
    json!({
        "issuer": issuer.as_str(),
        "authorization_endpoint": issuer.endpoint("/oauth/authorize"),
        "token_endpoint": issuer.endpoint("/oauth/token"),
        "registration_endpoint": issuer.endpoint("/oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": [SCOPE],
        "client_id_metadata_document_supported": true,
        "authorization_response_iss_parameter_supported": true,
    })
}

/// Why `/mcp` answers `401`: no `Authorization` header (`auth.missing`), or
/// a credential that was presented and failed (`auth.invalid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeCause {
    Missing,
    CredentialFailed,
}

/// The `WWW-Authenticate` value of a `401` from `/mcp` (ADR-0031 d2): the
/// RFC 9728 pointer and the scope, with `error="invalid_token"` (RFC 6750
/// §3.1) added only when a credential failed; `Bearer realm="oml"` where
/// `oauth.issuer` is unset. No other route's `401`, and no `200`, carries it.
#[must_use]
pub fn mcp_challenge(issuer: Option<&Issuer>, cause: ChallengeCause) -> String {
    let Some(issuer) = issuer else {
        return r#"Bearer realm="oml""#.to_owned();
    };
    let mut challenge = format!(
        r#"Bearer resource_metadata="{}", scope="{SCOPE}""#,
        issuer.protected_resource_metadata_url()
    );
    if cause == ChallengeCause::CredentialFailed {
        challenge.push_str(r#", error="invalid_token""#);
    }
    challenge
}

/// A `resource` parameter, at authorize or token: absent binds the token to
/// the one resource anyway; present, it must be exactly `<issuer>/mcp`,
/// else `oauth.resource_mismatch` (RFC 8707 §2; ADR-0031 d4).
pub fn check_resource(sent: Option<&str>, issuer: &Issuer) -> Result<(), Refusal> {
    match sent {
        Some(resource) if resource != issuer.resource() => Err(Refusal::ResourceMismatch),
        _ => Ok(()),
    }
}
