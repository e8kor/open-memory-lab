//! The sign-in providers' outbound calls, the lead's [`SignInExchange`]
//! (ADR-0045 d3, d4; ADR-0019: outbound I/O lives in an adapter).
//!
//! Google: the code, the PKCE verifier and the client secret go to Google's
//! token endpoint, and the answer's `id_token` comes back for the
//! application to judge. GitHub: the same to GitHub's token endpoint, then
//! `GET /user` for the account's numeric `id` and `GET /user/emails` for
//! its addresses. The provider's access token is dropped when the call
//! returns; nothing here stores it. The endpoints are constants, never
//! settings (ADR-0045 d17). Each call reads no proxy from the environment,
//! follows no redirect, and is cut at [`CALL_TIMEOUT`] and [`BODY_MAX_BYTES`];
//! TLS verifies the certificate against the web PKI roots. The adapter
//! judges nothing: an error answer, a timeout or a body it cannot read is
//! [`ProviderFailure::Refused`] with a closed reason.

use std::fmt;
use std::io::Read as _;
use std::time::Duration;

use oml_app::domain::web::{GitHubEmail, Provider};
use oml_app::ports::SignInExchange;
use oml_app::types::{ProviderAnswer, ProviderFailure};
use serde_json::Value;

/// Google's token endpoint.
pub const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
/// GitHub's token endpoint.
pub const GITHUB_TOKEN_ENDPOINT: &str = "https://github.com/login/oauth/access_token";
/// GitHub's account endpoint, for the numeric `id`.
pub const GITHUB_USER_ENDPOINT: &str = "https://api.github.com/user";
/// GitHub's email endpoint, for the primary verified address.
pub const GITHUB_EMAILS_ENDPOINT: &str = "https://api.github.com/user/emails";

/// The cut on one call, from connect to last byte (ADR-0045 d15).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// The cut on one answer's body.
pub const BODY_MAX_BYTES: usize = 64 * 1024;

/// One provider's registered application: its client id and secret, read
/// at boot. The secret is never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct ClientCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl fmt::Debug for ClientCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<set>")
            .finish()
    }
}

/// The lead's providers: each one's credentials where the deployment set
/// them. A provider without credentials answers every exchange refused.
#[derive(Debug, Clone, Default)]
pub struct ProviderClients {
    pub google: Option<ClientCredentials>,
    pub github: Option<ClientCredentials>,
}

impl SignInExchange for ProviderClients {
    async fn redeem(
        &self,
        provider: Provider,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<ProviderAnswer, ProviderFailure> {
        let credentials = match provider {
            Provider::Google => self.google.clone(),
            Provider::GitHub => self.github.clone(),
        }
        .ok_or(ProviderFailure::Refused("not_configured"))?;
        let (code, verifier, redirect_uri) = (
            code.to_owned(),
            verifier.to_owned(),
            redirect_uri.to_owned(),
        );
        tokio::task::spawn_blocking(move || match provider {
            Provider::Google => google(&credentials, &code, &verifier, &redirect_uri),
            Provider::GitHub => github(&credentials, &code, &verifier, &redirect_uri),
        })
        .await
        .unwrap_or(Err(ProviderFailure::Refused("task_failed")))
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(false)
        .redirects(0)
        .timeout(CALL_TIMEOUT)
        .build()
}

fn read_json(response: ureq::Response) -> Result<Value, ProviderFailure> {
    let mut body = Vec::new();
    response
        .into_reader()
        .take(BODY_MAX_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| ProviderFailure::Refused("unreadable_answer"))?;
    if body.len() > BODY_MAX_BYTES {
        return Err(ProviderFailure::Refused("answer_too_large"));
    }
    serde_json::from_slice(&body).map_err(|_| ProviderFailure::Refused("unreadable_answer"))
}

fn call_failed(error: &ureq::Error) -> ProviderFailure {
    match error {
        ureq::Error::Status(_, _) => ProviderFailure::Refused("error_answer"),
        ureq::Error::Transport(_) => ProviderFailure::Refused("unreachable"),
    }
}

fn google(
    credentials: &ClientCredentials,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<ProviderAnswer, ProviderFailure> {
    let response = agent()
        .post(GOOGLE_TOKEN_ENDPOINT)
        .set("Accept", "application/json")
        .send_form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &credentials.client_id),
            ("client_secret", &credentials.client_secret),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .map_err(|e| call_failed(&e))?;
    google_answer(&read_json(response)?)
}

/// The ID token of Google's token answer (OpenID Connect Core 1.0 §3.1.3.3).
pub fn google_answer(answer: &Value) -> Result<ProviderAnswer, ProviderFailure> {
    answer
        .get("id_token")
        .and_then(Value::as_str)
        .map(|token| ProviderAnswer::GoogleIdToken(token.to_owned()))
        .ok_or(ProviderFailure::Refused("no_id_token"))
}

fn github(
    credentials: &ClientCredentials,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<ProviderAnswer, ProviderFailure> {
    let agent = agent();
    let response = agent
        .post(GITHUB_TOKEN_ENDPOINT)
        .set("Accept", "application/json")
        .send_form(&[
            ("client_id", &credentials.client_id),
            ("client_secret", &credentials.client_secret),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .map_err(|e| call_failed(&e))?;
    let token = read_json(response)?
        .get("access_token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or(ProviderFailure::Refused("no_access_token"))?;
    let authorization = format!("Bearer {token}");
    let api = |url: &str| -> Result<Value, ProviderFailure> {
        let response = agent
            .get(url)
            .set("Accept", "application/vnd.github+json")
            .set("Authorization", &authorization)
            .set("User-Agent", "open-memory-lab")
            .set("X-GitHub-Api-Version", "2022-11-28")
            .call()
            .map_err(|e| call_failed(&e))?;
        read_json(response)
    };
    let account_id = api(GITHUB_USER_ENDPOINT)?
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(ProviderFailure::Refused("no_account_id"))?;
    let emails = github_emails(&api(GITHUB_EMAILS_ENDPOINT)?)?;
    Ok(ProviderAnswer::GitHub { account_id, emails })
}

/// GitHub's `GET /user/emails` answer as its entries; an entry missing a
/// field is read as unverified and not primary, so it never signs in.
pub fn github_emails(answer: &Value) -> Result<Vec<GitHubEmail>, ProviderFailure> {
    let entries = answer
        .as_array()
        .ok_or(ProviderFailure::Refused("unreadable_emails"))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(GitHubEmail {
                email: entry.get("email")?.as_str()?.to_owned(),
                primary: entry.get("primary").and_then(Value::as_bool) == Some(true),
                verified: entry.get("verified").and_then(Value::as_bool) == Some(true),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_github_entry_missing_its_flags_is_neither_primary_nor_verified() {
        let emails = github_emails(&json!([
            {"email": "a@example.com", "primary": true, "verified": true},
            {"email": "b@example.com"},
        ]))
        .expect("an array");
        assert_eq!(emails.len(), 2);
        assert!(emails[0].primary && emails[0].verified);
        assert!(!emails[1].primary && !emails[1].verified);
    }

    #[test]
    fn a_google_answer_without_an_id_token_is_refused() {
        assert_eq!(
            google_answer(&json!({"access_token": "x"})),
            Err(ProviderFailure::Refused("no_id_token"))
        );
    }

    #[test]
    fn the_client_secret_is_never_printed() {
        let credentials = ClientCredentials {
            client_id: "id".to_owned(),
            client_secret: "s3cr3t".to_owned(),
        };
        assert!(!format!("{credentials:?}").contains("s3cr3t"));
    }
}
