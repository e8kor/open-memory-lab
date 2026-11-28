//! The operator's host route (ADR-0097 d19; answer 13 of 2026-11-27):
//! `POST /v2/operator/tenants/{tenant}/acts/{act}/settle`, which `oml
//! operator settle`, run as root on the host, calls with the per-tenant
//! token the server wrote at its start to a 0600 file. It admits that token
//! alone: any bearer key, any other token and a token of another tenant
//! are refused, and no admin key is involved. The settlement's email goes
//! to the mail relay after the commit (d20; answer 14).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::OutboundMail;

use crate::admin_acts::admin_act_json;
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, finish, json_response, parse_json, retry_chain};

/// The header that carries the operator's token. Never logged: the request
/// line carries no header but ADR-0042 d9's.
pub const OPERATOR_TOKEN_HEADER: &str = "oml-operator-token";

/// How long the route waits on the mail relay after the commit: 10 s,
/// chosen, not measured. Past it the delivery counts as failed and the
/// settlement stands (d20; answer 14).
pub const MAIL_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The operator tokens of this process's start, one per tenant, held as
/// their SHA-256 digests only (answer 13): a restart writes new ones, so a
/// token outlives no process.
#[derive(Debug, Default)]
pub struct OperatorTokens {
    digests: HashMap<Uuid, Vec<u8>>,
    /// The start could not write them: `/readyz` says `failed`, which
    /// doctor names `operator.token_unwritten`.
    failed: bool,
}

impl OperatorTokens {
    /// The tokens `tokens` names, by tenant.
    #[must_use]
    pub fn new(tokens: impl IntoIterator<Item = (Uuid, String)>) -> Self {
        Self {
            digests: tokens
                .into_iter()
                .map(|(tenant, token)| (tenant, digest(&token)))
                .collect(),
            failed: false,
        }
    }

    /// No token, because the start could not write them.
    #[must_use]
    pub fn failed() -> Self {
        Self {
            digests: HashMap::new(),
            failed: true,
        }
    }

    /// What `/readyz` says of them: `written`, `none` (no tenant, or a
    /// composition that writes none), or `failed`.
    #[must_use]
    pub fn state(&self) -> &'static str {
        if self.failed {
            "failed"
        } else if self.digests.is_empty() {
            "none"
        } else {
            "written"
        }
    }

    /// How many tenants hold a token.
    #[must_use]
    pub fn len(&self) -> usize {
        self.digests.len()
    }

    /// Whether no tenant holds a token.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }

    /// Whether `token` is `tenant`'s token of this start.
    #[must_use]
    pub fn admits(&self, tenant: Uuid, token: &str) -> bool {
        self.digests
            .get(&tenant)
            // Digests compared, never the token: a comparison's timing
            // tells of the digest alone.
            .is_some_and(|d| *d == digest(token))
    }
}

fn digest(token: &str) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .to_vec()
}

/// What the mail relay did since the start (d20): whether one is
/// configured, how many deliveries failed and the last failure's reason,
/// which doctor reads as `notice.mail_unconfigured` and
/// `notice.mail_delivery_failed`. The count is the process's: a restart
/// clears it.
#[derive(Debug, Default)]
pub struct MailHealth {
    failures: AtomicU64,
    last_failure: Mutex<Option<String>>,
}

impl MailHealth {
    /// Records a failed delivery and its reason.
    pub fn failed(&self, reason: &str) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = self.last_failure.lock() {
            *last = Some(reason.to_owned());
        }
    }

    /// The failed deliveries since the start.
    #[must_use]
    pub fn failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    /// The last failure's reason, if any.
    #[must_use]
    pub fn last_failure(&self) -> Option<String> {
        self.last_failure.lock().ok().and_then(|l| l.clone())
    }
}

/// The body of the recover route: the admin person the recovery adds and
/// the address of their entry (ADR-0097 d19).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverBody {
    person: String,
    email: String,
}

/// What one pass of the recovery sweep did over every tenant it was given
/// (ADR-0097 d19, answer 22): the lead logs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoverySweepReport {
    pub completed: u64,
    pub cancelled: u64,
    pub purged: u64,
    /// The tenants whose pass failed, each with its reason; the next pass
    /// runs them again.
    pub failed: Vec<String>,
}

/// A recovery as the route and the sweep answer it.
fn recovery_json(recovery: &oml_app::types::RecoveryRecord) -> Value {
    json!({
        "recovery_id": recovery.recovery_id.to_string(),
        "person": recovery.person.as_str(),
        "state": recovery.state.as_str(),
        "requested_at": recovery.requested_at.as_str(),
        "completes_after": recovery.completes_after().map(|t| t.as_str().to_owned()),
        "settled_at": recovery.settled_at.as_ref().map(|t| t.as_str().to_owned()),
    })
}

/// The body of the settle route: `confirm` or `reverse`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleBody {
    outcome: String,
}

/// The `auth.invalid` the route answers for anything but the tenant's
/// token: `reason` names which.
fn refused(reason: &str) -> ApiError {
    ApiError::auth_invalid(reason)
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `POST /v2/operator/tenants/{tenant}/acts/{act}/settle {outcome}`
    /// (ADR-0097 d19): the tenant's operator token in `OML-Operator-Token`
    /// and no `Authorization` header. A request that sends a key is
    /// `auth.invalid` `operator_route_takes_no_key`; no token, `auth.missing`; a token
    /// that is not the tenant's of this start, `auth.invalid`
    /// `operator_token`. Then the settlement, and its email to the relay.
    pub(crate) async fn operator_settle_route(
        State(st): State<Self>,
        rid: Rid,
        Path((tenant, act)): Path<(String, String)>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            // No token is no credential at all, as on every other route.
            let tenant = st.operator_tenant(&headers, &tenant)?;
            st.require_writes_open()?;
            let act = Uuid::parse_str(&act).map_err(|_| ApiError::not_found())?;
            let sent: SettleBody = parse_json(&headers, &body)?;
            let confirm = match sent.outcome.as_str() {
                "confirm" => true,
                "reverse" => false,
                _ => {
                    return Err(ApiError::validation("outcome is confirm or reverse")
                        .with_details(json!({ "field": "/outcome" })));
                }
            };
            let settled = retry_chain(|| st.app.operator_settle(tenant, act, confirm))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let (mail, failure) = st.send_mail(&settled.mail).await;
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "act": admin_act_json(&settled.act),
                    "notified": settled.notified.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                    "mail": mail,
                    "mail_failure": failure,
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/operator/tenants/{tenant}/recover {person, email}`
    /// (ADR-0097 d19; answers 21, 22, 24): the tenant's operator token, as
    /// the settle route takes it, and no key. The request is admitted only
    /// over 30 days the authentication table covers with no admin
    /// authentication; it is journalled, every admin person is notified and
    /// emailed, and the server's sweep completes it after 72 hours with no
    /// admin sign-in.
    pub(crate) async fn operator_recover_route(
        State(st): State<Self>,
        rid: Rid,
        Path(tenant): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            let tenant = st.operator_tenant(&headers, &tenant)?;
            st.require_writes_open()?;
            let sent: RecoverBody = parse_json(&headers, &body)?;
            let person = oml_app::domain::name::Name::parse(sent.person.trim()).map_err(|e| {
                ApiError::validation(format!("person: {e}"))
                    .with_details(json!({ "field": "/person" }))
            })?;
            let email = oml_app::domain::web::Email::parse(sent.email.trim()).map_err(|_| {
                ApiError::validation("email is not an email address")
                    .with_details(json!({ "field": "/email" }))
            })?;
            let requested = retry_chain(|| {
                st.app
                    .operator_recover(tenant, person.clone(), email.clone())
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let (mail, failure) = st.send_mail(&requested.mail).await;
            Ok(json_response(
                StatusCode::ACCEPTED,
                &json!({
                    "recovery": recovery_json(&requested.recovery),
                    "notified": requested.notified.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                    "mail": mail,
                    "mail_failure": failure,
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// The tenant the operator route names, once its token is the tenant's
    /// of this start: a request that sends a key is `auth.invalid`
    /// `operator_route_takes_no_key`, no token `auth.missing`, and any other
    /// token `auth.invalid` `operator_token` (answer 13).
    fn operator_tenant(&self, headers: &HeaderMap, tenant: &str) -> Result<Uuid, ApiError> {
        if headers.contains_key(header::AUTHORIZATION) {
            return Err(refused("operator_route_takes_no_key"));
        }
        let token = headers
            .get(OPERATOR_TOKEN_HEADER)
            .ok_or_else(ApiError::auth_missing)?
            .to_str()
            .map_err(|_| refused("operator_token"))?;
        let tenant = Uuid::parse_str(tenant).map_err(|_| refused("operator_token"))?;
        if !self.operator.admits(tenant, token.trim()) {
            return Err(refused("operator_token"));
        }
        Ok(tenant)
    }

    /// One pass of the server's recovery sweep over `tenants` (ADR-0097
    /// d19, answers 22, 24): each tenant's authentication rows older than
    /// 30 days purged, its requested recovery cancelled by an admin sign-in
    /// or completed after 72 hours, and each completion's email handed to
    /// the relay. A pass that meets another replica's commit runs again and
    /// finds the recovery settled, so a request completes once.
    pub async fn recovery_sweep(&self, tenants: &[Uuid]) -> RecoverySweepReport {
        let mut report = RecoverySweepReport::default();
        for &tenant in tenants {
            match self.app.purge_authentications(tenant).await {
                Ok(n) => report.purged += n,
                Err(e) => report.failed.push(format!("tenant {tenant} purge: {e}")),
            }
            match retry_chain(|| self.app.recovery_sweep(tenant)).await {
                Ok(sweep) => {
                    report.cancelled += sweep.cancelled;
                    for done in sweep.completed {
                        report.completed += 1;
                        let _ = self.send_mail(&done.mail).await;
                    }
                }
                Err(e) => report.failed.push(format!("tenant {tenant} recovery: {e}")),
            }
        }
        report
    }

    /// Hands `mail` to the relay, if one is configured (d20; answer 14):
    /// `sent`, `unconfigured`, or `failed` with its reason, which doctor
    /// then reports. The act it tells about stands either way.
    pub(crate) async fn send_mail(&self, mail: &OutboundMail) -> (&'static str, Value) {
        let Some(mailer) = &self.mailer else {
            return ("unconfigured", Value::Null);
        };
        if mail.to.is_empty() {
            return ("sent", Value::Null);
        }
        let reason = match tokio::time::timeout(MAIL_SEND_TIMEOUT, mailer.send(mail)).await {
            Ok(Ok(())) => return ("sent", Value::Null),
            Ok(Err(reason)) => reason,
            Err(_) => format!(
                "the relay did not answer within {} s",
                MAIL_SEND_TIMEOUT.as_secs()
            ),
        };
        self.mail_health.failed(&reason);
        tracing::error!(
            event = "notice_mail_failed",
            reason = %reason,
            "the mail relay refused or did not take a notice email; the act stands and doctor reports notice.mail_delivery_failed"
        );
        ("failed", Value::String(reason))
    }

    /// The mail lines' counts for the doctor report (d20).
    pub(crate) fn mail_report(&self) -> Value {
        json!({
            "mail_unconfigured": u64::from(self.mailer.is_none()),
            "mail_delivery_failures": self.mail_health.failures(),
            "mail_last_failure": self.mail_health.last_failure(),
        })
    }
}
