//! CI's sign-in double, compiled only with the `e2e-sign-in` feature
//! (ADR-0070, phase (b) as built): the docker profile, `oml quickstart` and
//! the chart's CI jobs sign a person in through the real routes, then load a
//! page `oml-web` serves through the router. The provider endpoints are
//! constants (ADR-0045 d17) that no CI runner can answer, so this build
//! answers every GitHub exchange with one verified primary address, which
//! the allowlist must still admit. No release build carries it
//! (`scripts/check_release.py`).

use oml_app::domain::web::{GitHubEmail, Provider};
use oml_app::ports::SignInExchange;
use oml_app::types::{ProviderAnswer, ProviderFailure};

/// The environment variable naming the address every GitHub sign-in
/// admits.
pub const EMAIL_VAR: &str = "OML_E2E_SIGN_IN_EMAIL";

/// The double: GitHub answers `email`, verified and primary, under one fixed
/// account id; Google is refused.
pub struct E2eSignIn {
    pub email: String,
}

impl SignInExchange for E2eSignIn {
    async fn redeem(
        &self,
        provider: Provider,
        _code: &str,
        _verifier: &str,
        _redirect_uri: &str,
    ) -> Result<ProviderAnswer, ProviderFailure> {
        match provider {
            Provider::GitHub => Ok(ProviderAnswer::GitHub {
                account_id: 4_242,
                emails: vec![GitHubEmail {
                    email: self.email.clone(),
                    primary: true,
                    verified: true,
                }],
            }),
            Provider::Google => Err(ProviderFailure::Refused("e2e_double")),
        }
    }
}
