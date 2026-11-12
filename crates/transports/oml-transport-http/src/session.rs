//! The capacity credential at the API (ADR-0070 d2): `Authorization: Bearer
//! omls_…` names one of the person's session keys through a live web
//! session, and only from `oml-web`'s client identity. On `/mcp` it is
//! refused always; on `/v1` and `/v2` it is refused unless the request
//! proves the identity the lead is configured for, the web-client secret
//! sent as `OML-Web-Client` or a client certificate the lead's own TLS
//! listener verified (`auth.invalid`, `details.reason: session_outside_web`).
//! A client certificate a gateway forwards in a header is a field, not an
//! identity: a request carrying one is refused the same way.
//!
//! The gate resolves the credential to its session key's caller once per
//! request, re-checked as a page's session is today (the session is live,
//! its entry names the user, the admin capacity only for an admin's
//! entry), and the handlers act through it as through any key
//! (`Acting::Session`). Neither the credential nor the secret is
//! logged.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, header};
use axum::middleware::Next;
use axum::response::Response;

use oml_app::app::Caller;
use oml_app::domain::web::{CAPACITY_CREDENTIAL_PREFIX, DOCTOR_PROBE_CREDENTIAL, constant_time_eq};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{AppError, WebSessionRecord};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, finish};

/// The header `oml-web` sends its client secret in (ADR-0070 d2).
pub const WEB_CLIENT_HEADER: &str = "oml-web-client";

/// The headers a gateway forwards a client certificate in. A capacity
/// credential that arrives beside one is refused: the lead trusts only a
/// certificate its own listener verified (ADR-0070 d2).
pub const FORWARDED_CERTIFICATE_HEADERS: [&str; 6] = [
    "x-forwarded-client-cert",
    "x-client-cert",
    "x-client-certificate",
    "x-ssl-client-cert",
    "ssl-client-cert",
    "client-cert",
];

/// `details.reason` of a capacity credential refused outside `oml-web`.
pub const SESSION_OUTSIDE_WEB: &str = "session_outside_web";

/// How `oml-web` proves itself to the lead (ADR-0070 d2): the secret set
/// on both processes (`web.client_secret`), or a client certificate under
/// `web.client_ca` whose subject is `web.client_subject`. Without either,
/// every capacity credential is refused and the doctor warns. During a
/// rotation the lead also accepts `web.client_secret_previous` until its
/// window closes (#230), so `oml-web` replicas still sending it keep
/// working while they restart onto the new one.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct WebClient {
    secret: Option<String>,
    subject: Option<String>,
    /// The previous secret and the instant, in microseconds since the
    /// epoch, from which it proves nothing.
    previous: Option<(String, i64)>,
}

impl std::fmt::Debug for WebClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebClient")
            .field("secret", &self.secret.as_ref().map(|_| "<set>"))
            .field("subject", &self.subject)
            .field(
                "previous_until",
                &self.previous.as_ref().map(|(_, until)| until),
            )
            .finish()
    }
}

impl WebClient {
    /// The identity the lead accepts: the secret, the certificate subject,
    /// or both.
    #[must_use]
    pub fn new(secret: Option<String>, subject: Option<String>) -> Self {
        Self {
            secret: secret.filter(|s| !s.is_empty()),
            subject: subject.filter(|s| !s.is_empty()),
            previous: None,
        }
    }

    /// Also accepts `previous` until `until_us` (microseconds since the
    /// epoch), and never from that instant on (#230).
    #[must_use]
    pub fn with_previous(mut self, previous: String, until_us: i64) -> Self {
        self.previous = Some((previous, until_us)).filter(|(p, _)| !p.is_empty());
        self
    }

    /// When the previous secret stops proving anything, in microseconds
    /// since the epoch, while one is set.
    #[must_use]
    pub fn previous_until(&self) -> Option<i64> {
        self.previous.as_ref().map(|(_, until)| *until)
    }

    /// `secret`, `mtls`, both joined by `+`, or `none`: what `/readyz`
    /// names for the doctor line (ADR-0070 Consequences). A previous
    /// secret changes nothing here: it is a rotation of `secret`.
    #[must_use]
    pub fn identity(&self) -> &'static str {
        match (&self.secret, &self.subject) {
            (Some(_), Some(_)) => "secret+mtls",
            (Some(_), None) => "secret",
            (None, Some(_)) => "mtls",
            (None, None) => "none",
        }
    }

    /// Whether the request proves `oml-web`'s identity at `now_us`: the
    /// secret, or the previous one before its window closes, each compared
    /// in constant time, or a certificate the listener verified with the
    /// configured subject; never with a forwarded certificate header.
    fn proven(
        &self,
        headers: &HeaderMap,
        peer: Option<&VerifiedClientCertificate>,
        now_us: i64,
    ) -> bool {
        if FORWARDED_CERTIFICATE_HEADERS
            .iter()
            .any(|h| headers.contains_key(*h))
        {
            return false;
        }
        let sent = headers.get(WEB_CLIENT_HEADER).map(|v| v.as_bytes());
        let by_secret = self.secret.as_ref().is_some_and(|secret| {
            sent.is_some_and(|sent| constant_time_eq(sent, secret.as_bytes()))
        });
        let by_previous = self.previous.as_ref().is_some_and(|(previous, until)| {
            now_us < *until && sent.is_some_and(|sent| constant_time_eq(sent, previous.as_bytes()))
        });
        let by_certificate = self
            .subject
            .as_ref()
            .is_some_and(|subject| peer.is_some_and(|p| &p.subject == subject));
        by_secret || by_previous || by_certificate
    }
}

/// Now, in microseconds since the epoch, for the previous secret's window.
fn now_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_micros()).unwrap_or(i64::MAX))
}

/// The subject of a client certificate the lead's own TLS listener
/// verified against `web.client_ca`, set by that listener as a request
/// extension; nothing a client sends can set it.
#[derive(Debug, Clone)]
pub struct VerifiedClientCertificate {
    pub subject: String,
}

/// What the gate resolved for this request: the session key's caller and
/// the session its credential derives from.
#[derive(Clone)]
struct Resolved {
    caller: Arc<Caller>,
    session: Arc<WebSessionRecord>,
}

tokio::task_local! {
    static SESSION: Resolved;
}

/// The session key's caller of this request, where a capacity credential
/// resolved one.
pub(crate) fn session_caller() -> Option<Arc<Caller>> {
    SESSION.try_with(|r| Arc::clone(&r.caller)).ok()
}

/// The running request's resolved session, to carry onto a task the
/// request spawns and awaits (#233).
#[derive(Clone)]
pub(crate) struct Carried(Option<Resolved>);

/// The running request's session, as [`within_session`] carries it.
pub(crate) fn current_session() -> Carried {
    Carried(SESSION.try_with(Clone::clone).ok())
}

/// Runs `work` with the session `carried` holds, as the request that
/// captured it would.
pub(crate) async fn within_session<F: std::future::Future>(carried: Carried, work: F) -> F::Output {
    match carried.0 {
        Some(resolved) => SESSION.scope(resolved, work).await,
        None => work.await,
    }
}

/// The session this request's capacity credential derives from, where it
/// carries one.
pub(crate) fn session_record() -> Option<Arc<WebSessionRecord>> {
    SESSION.try_with(|r| Arc::clone(&r.session)).ok()
}

/// The capacity credential in `Authorization`, when it carries one.
fn capacity_credential(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|t| t.starts_with(CAPACITY_CREDENTIAL_PREFIX))
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The gate every route passes: a request without a capacity
    /// credential passes untouched; one with it is refused on `/mcp` and
    /// outside `oml-web`'s identity, and otherwise runs as its session
    /// key's caller.
    pub(crate) async fn session_gate(State(st): State<Self>, req: Request, next: Next) -> Response {
        let Some(credential) = capacity_credential(req.headers()).map(str::to_owned) else {
            return next.run(req).await;
        };
        let rid = Rid(req
            .extensions()
            .get::<crate::traffic::RequestIds>()
            .cloned()
            .unwrap_or_else(|| crate::traffic::RequestIds::of(req.headers())));
        let path = req.uri().path();
        let mcp = path == "/mcp" || path.starts_with("/mcp/");
        let ring = path.starts_with("/v1/") || path.starts_with("/v2/");
        if !mcp && !ring {
            // The probes, the OAuth routes and the pages read no key from
            // `Authorization`; each answers it as it answers any value.
            return next.run(req).await;
        }
        if mcp
            || !st
                .web_client
                .proven(req.headers(), req.extensions().get(), now_micros())
        {
            // A capacity credential reaches the API only through oml-web,
            // and never `/mcp` (ADR-0070 d2; ADR-0045 d7).
            crate::traffic::credential(None, None);
            return finish(&rid, Err(ApiError::auth_invalid(SESSION_OUTSIDE_WEB)));
        }
        if credential == DOCTOR_PROBE_CREDENTIAL {
            // `oml-web --doctor`'s probe, proven: answered as any credential
            // no session holds, and neither logged nor counted (#262).
            let mut answer = finish(&rid, Err(ApiError::auth_invalid("unknown")));
            answer.extensions_mut().insert(crate::traffic::Unrecorded);
            return answer;
        }
        match st.app.web_credential_caller(&credential).await {
            Ok((caller, session)) => {
                let resolved = Resolved {
                    caller: Arc::new(caller),
                    session: Arc::new(session),
                };
                SESSION.scope(resolved, next.run(req)).await
            }
            Err(e @ (AppError::Store(_) | AppError::KeyRateLimited { .. })) => {
                finish(&rid, Err(map_app_error(e, ErrorContext::Write)))
            }
            Err(_) => finish(&rid, Err(ApiError::auth_invalid("unknown"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    /// An instant for the gate's clock, in microseconds since the epoch.
    const NOW: i64 = 1_800_000_000_000_000;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_str(value).expect("a header value"));
        }
        headers
    }

    /// oml-web is proven by the secret, compared whole, or by a certificate
    /// the lead's own listener verified with the configured subject; never
    /// by a certificate header, and never by nothing (ADR-0070 d2).
    #[test]
    fn oml_web_is_proven_by_its_secret_or_a_verified_certificate_never_by_a_header() {
        let secret = WebClient::new(Some("the-secret".to_owned()), None);
        assert!(secret.proven(&headers(&[(WEB_CLIENT_HEADER, "the-secret")]), None, NOW));
        assert!(!secret.proven(&headers(&[(WEB_CLIENT_HEADER, "the-secre")]), None, NOW));
        assert!(!secret.proven(&headers(&[]), None, NOW));
        for forwarded in FORWARDED_CERTIFICATE_HEADERS {
            assert!(
                !secret.proven(
                    &headers(&[(WEB_CLIENT_HEADER, "the-secret"), (forwarded, "CN=oml-web")]),
                    None,
                    NOW
                ),
                "{forwarded}"
            );
        }
        let mtls = WebClient::new(None, Some("CN=oml-web".to_owned()));
        let verified = VerifiedClientCertificate {
            subject: "CN=oml-web".to_owned(),
        };
        let other = VerifiedClientCertificate {
            subject: "CN=someone".to_owned(),
        };
        assert!(mtls.proven(&headers(&[]), Some(&verified), NOW));
        assert!(!mtls.proven(&headers(&[]), Some(&other), NOW));
        assert!(!mtls.proven(&headers(&[(WEB_CLIENT_HEADER, "the-secret")]), None, NOW));
        assert!(!WebClient::default().proven(&headers(&[(WEB_CLIENT_HEADER, "")]), None, NOW));
        assert_eq!(WebClient::default().identity(), "none");
        assert!(!format!("{secret:?}").contains("the-secret"));
    }

    /// Rotation without downtime (#230): the previous secret proves
    /// `oml-web` beside the current one until its window closes, and never
    /// after; the identity `/readyz` names stays `secret`, and neither
    /// value is in the debug form.
    #[test]
    fn the_previous_secret_proves_oml_web_only_until_its_window_closes() {
        let until = NOW + 60_000_000;
        let rotating = WebClient::new(Some("the-new-secret".to_owned()), None)
            .with_previous("the-old-secret".to_owned(), until);
        let old = headers(&[(WEB_CLIENT_HEADER, "the-old-secret")]);
        let new = headers(&[(WEB_CLIENT_HEADER, "the-new-secret")]);
        assert!(rotating.proven(&new, None, NOW));
        assert!(rotating.proven(&old, None, NOW));
        assert!(rotating.proven(&old, None, until - 1));
        assert!(!rotating.proven(&old, None, until), "closed at the instant");
        assert!(rotating.proven(&new, None, until + 1));
        assert!(!rotating.proven(&headers(&[(WEB_CLIENT_HEADER, "the-old-secre")]), None, NOW));
        for forwarded in FORWARDED_CERTIFICATE_HEADERS {
            assert!(
                !rotating.proven(
                    &headers(&[
                        (WEB_CLIENT_HEADER, "the-old-secret"),
                        (forwarded, "CN=oml-web")
                    ]),
                    None,
                    NOW
                ),
                "{forwarded}"
            );
        }
        assert_eq!(rotating.identity(), "secret");
        assert_eq!(rotating.previous_until(), Some(until));
        let shown = format!("{rotating:?}");
        assert!(
            !shown.contains("the-old-secret") && !shown.contains("the-new-secret"),
            "{shown}"
        );
    }
}
