//! Open sign-up's pages (ADR-0068): the terms page and its act, part of
//! the auth context (ADR-0070 d6). The onboarding page, its status poll,
//! the export and the admin's users page are `oml-web`'s (ADR-0070 d7 (b)).

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

use axum::Extension;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};

use oml_app::app::CallbackRefusal;
use oml_app::domain::web::{Email, Provider, ProviderIdentity, SIGN_IN_TTL_SECS, SignInRefusal};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::AppError;

use super::page::{STYLESHEET, attr, shell, short};
use super::{
    Answer, append_cookie, clear_cookie, cookie, human_only, origin_refused, set_cookie,
    unique_fields, unix_now,
};
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::oauth::{form_pairs, require_media, same_origin};
use crate::traffic::{ClientAddr, Rid, WebLine};
use crate::{ApiState, retry_chain};
use oml_app::ports::HandleSealer as _;
use oml_page::digest::{base64_32, sha256};
use oml_page::escape;

/// The pending sign-up's cookie: the admitted identity, sealed, for the
/// terms page's ten minutes (ADR-0068 d2).
pub const SIGN_UP_COOKIE: &str = "__Host-oml_signup";

// The stylesheet is long enough that hashing it runs past the compiler's
// const-evaluation lint; the work is bounded by the file's size.
#[allow(long_running_const_eval)]
const STYLE_DIGEST: [u8; 44] = base64_32(&sha256(STYLESHEET.as_bytes()));

const FORM: &str = "application/x-www-form-urlencoded";

/// The CSP of these pages: the one stylesheet, forms to the page alone,
/// no script and no frame.
#[must_use]
pub fn start_csp() -> String {
    format!(
        "default-src 'none'; style-src 'sha256-{}'; form-action 'self'; \
         frame-ancestors 'none'; base-uri 'none'",
        std::str::from_utf8(&STYLE_DIGEST).expect("base64 is ASCII")
    )
}

fn secured(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&start_csp()).expect("the CSP is ASCII"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn html_response(status: StatusCode, html: String) -> Response {
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secured(response)
}

/// A page of these: the shared chrome of `page::shell` and the design
/// system's one stylesheet.
fn document(
    title: &str,
    who: Option<&str>,
    tabs: &[(&str, &str)],
    active: &str,
    body: &str,
) -> String {
    shell(title, who.unwrap_or(""), None, tabs, active, None, body)
}

/// The pending sign-up the cookie seals (ADR-0068 d2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSignUp {
    pub identity: ProviderIdentity,
    pub started_at: i64,
}

impl PendingSignUp {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        json!({
            "p": self.identity.provider.as_str(),
            "s": self.identity.subject,
            "e": self.identity.email.as_str(),
            "t": self.started_at,
        })
        .to_string()
        .into_bytes()
    }

    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let v: Value = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            identity: ProviderIdentity {
                provider: Provider::parse(v.get("p")?.as_str()?)?,
                subject: v.get("s")?.as_str()?.to_owned(),
                email: Email::parse(v.get("e")?.as_str()?).ok()?,
            },
            started_at: v.get("t")?.as_i64()?,
        })
    }

    fn live(&self, now: i64) -> bool {
        now >= self.started_at && now - self.started_at < SIGN_IN_TTL_SECS
    }
}

/// Sign-ups per client address in a rolling hour (ADR-0068 d9): the
/// process's own count, one bucket per client, and one shared bucket where
/// the address is unknown. A client is an IPv4 address or an IPv6 /64, the
/// smallest network a host can hold whole, so one host cannot draw a new
/// bucket from its own prefix; an IPv4-mapped IPv6 address is its IPv4 one.
#[derive(Debug, Default)]
pub struct SignupLimiter(std::sync::Mutex<HashMap<Option<IpAddr>, VecDeque<i64>>>);

/// The bucket of a client address: IPv4 by itself, IPv6 by its /64.
fn client_bucket(addr: Option<IpAddr>) -> Option<IpAddr> {
    addr.map(|a| match a.to_canonical() {
        IpAddr::V6(v6) => {
            let high = u128::from(v6) & (u128::MAX << 64);
            IpAddr::V6(high.into())
        }
        v4 => v4,
    })
}

impl SignupLimiter {
    /// Takes one slot for `addr` at `now`, or the seconds until one frees.
    pub fn take(&self, addr: Option<IpAddr>, now: i64, per_hour: u64) -> Result<(), u64> {
        let mut map = self.0.lock().expect("signup limiter lock");
        // A bucket whose newest slot has aged out is dropped, so the map
        // holds only the clients seen in the last hour.
        map.retain(|_, slots| slots.back().is_some_and(|t| now - t < 3600));
        let slots = map.entry(client_bucket(addr)).or_default();
        while slots.front().is_some_and(|t| now - t >= 3600) {
            slots.pop_front();
        }
        if slots.len() as u64 >= per_hour {
            let oldest = slots.front().copied().unwrap_or(now);
            return Err(u64::try_from(3600 - (now - oldest)).unwrap_or(1).max(1));
        }
        slots.push_back(now);
        Ok(())
    }

    #[cfg(test)]
    fn buckets(&self) -> usize {
        self.0.lock().expect("signup limiter lock").len()
    }
}

fn app_error(e: AppError) -> ApiError {
    map_app_error(e, ErrorContext::Write)
}

fn field_error(field: &str, message: &str) -> ApiError {
    ApiError::validation(message.to_owned()).with_details(json!({ "field": field }))
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    fn start_refused(error: &ApiError, line: WebLine) -> Answer {
        let mut answer = Answer {
            response: html_response(
                error.status,
                document(
                    "Not available",
                    None,
                    &[],
                    "",
                    &format!(
                        r#"<h1>This cannot continue</h1><p class="error">{}: {}</p><p><a href="/web/start">Back</a></p>"#,
                        escape(error.code),
                        escape(&error.message)
                    ),
                ),
            ),
            error_code: Some(error.code),
            line,
        };
        error.mark_quota(&mut answer.response);
        answer
    }

    // -- the terms page --------------------------------------------------------

    /// The callback's answer for an unlisted identity in open mode
    /// (ADR-0068 d2): the identity sealed into the sign-up cookie, and the
    /// terms page. Nothing is written.
    pub(crate) fn terms_page(&self, identity: ProviderIdentity) -> Response {
        let Some(policy) = self.app.signup_policy() else {
            return html_response(StatusCode::NOT_FOUND, String::new());
        };
        let pending = PendingSignUp {
            identity,
            started_at: i64::try_from(unix_now()).unwrap_or(0),
        };
        let sealed = self.pages().signup_sealer.seal(&pending.encode());
        let body = format!(
            r#"<div class="signin"><p class="label">Create your account</p><h1>One step left</h1><p class="desc">A private space for your memories, a key for each agent, and a review page. Your agents' writes wait for your review; the operator of this server can read what you store.</p><form method="post" action="/web/sign-up"><input type="hidden" name="terms_version" value="{}"><label><input type="checkbox" name="accept_terms" value="yes" required> I accept the <a href="{}">terms</a> (version {}) and have read the <a href="{}">privacy notice</a>.</label><p class="oml-consequence">Creates your account and signs you in.</p><button type="submit" class="primary">Create my account</button></form></div>"#,
            attr(&policy.terms_version),
            attr(&policy.terms_url),
            short(&policy.terms_version),
            attr(&policy.privacy_url),
        );
        let mut response = html_response(
            StatusCode::OK,
            document("Create your account", None, &[], "", &body),
        );
        append_cookie(
            &mut response,
            &set_cookie(SIGN_UP_COOKIE, &sealed, SIGN_IN_TTL_SECS),
        );
        response
    }

    /// `POST /web/sign-up` (ADR-0068 d2, d9): the same-origin check, the
    /// sealed identity, the accepted current terms, one slot of the
    /// address's hourly limit; then the entry, the keys and a session,
    /// and `303 /web/start`.
    pub(crate) async fn sign_up(
        State(st): State<Self>,
        rid: Rid,
        addr: Option<Extension<ClientAddr>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let addr = addr.and_then(|Extension(a)| a.0);
        let answer = st.sign_up_inner(&rid, &headers, &body, addr).await;
        Self::web_finish(&rid, answer)
    }

    async fn sign_up_inner(
        &self,
        request_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
        addr: Option<IpAddr>,
    ) -> Answer {
        let line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return Self::start_refused(&refused, line);
        }
        let issuer = self.app.oauth_issuer().expect("routed only with an issuer");
        if !same_origin(headers, issuer) {
            return Self::start_refused(&origin_refused(), line);
        }
        if let Err(e) = require_media(headers, FORM) {
            return Self::start_refused(&ApiError::validation(e.description), line);
        }
        let fields: HashMap<String, String> =
            match form_pairs(body).and_then(|pairs| unique_fields(&pairs)) {
                Ok(f) => f.into_iter().collect(),
                Err(e) => return Self::start_refused(&ApiError::validation(e.description), line),
            };
        if let Some(name) = fields
            .keys()
            .find(|n| !matches!(n.as_str(), "accept_terms" | "terms_version"))
        {
            return Self::start_refused(
                &field_error(name, &format!("the terms page takes no field {name}")),
                line,
            );
        }
        let now = i64::try_from(unix_now()).unwrap_or(0);
        let Some(pending) = cookie(headers, SIGN_UP_COOKIE)
            .and_then(|sealed| self.pages().signup_sealer.open(&sealed))
            .and_then(|bytes| PendingSignUp::decode(&bytes))
            .filter(|p| p.live(now))
        else {
            let error = app_error(AppError::WebSignIn(SignInRefusal::StateMismatch));
            let mut answer = Self::start_refused(&error, line);
            append_cookie(&mut answer.response, &clear_cookie(SIGN_UP_COOKIE));
            return answer;
        };
        if fields.get("accept_terms").map(String::as_str) != Some("yes") {
            return Self::start_refused(
                &field_error("accept_terms", "accept the terms to create your account"),
                line,
            );
        }
        let Some(policy) = self.app.signup_policy() else {
            return Self::start_refused(&ApiError::not_found(), line);
        };
        let per_hour = policy.per_ip_per_hour;
        // A sign-up of an address an entry already holds is a sign-in,
        // and takes no slot (d9).
        let listed = self
            .store
            .web_entry_by_email(&pending.identity.email.digest())
            .await
            .ok()
            .flatten()
            .is_some();
        if !listed && let Err(retry) = self.pages().signups.take(addr, now, per_hour) {
            return Self::start_refused(
                &app_error(AppError::SignupRateLimited {
                    retry_after_secs: retry,
                }),
                line,
            );
        }
        let accepted = fields.get("terms_version").cloned().unwrap_or_default();
        let entry = match retry_chain(|| self.app.web_sign_up(&pending.identity, &accepted)).await {
            Ok(entry) => entry,
            Err(e) => {
                // An entry exists now: the cookie is spent, and the person
                // signs in through the callback (ADR-0068 d2).
                let spent = matches!(e, AppError::WebSignIn(_));
                let mut answer = Self::start_refused(&app_error(e), line);
                if spent {
                    append_cookie(&mut answer.response, &clear_cookie(SIGN_UP_COOKIE));
                }
                return answer;
            }
        };
        if let Err(e) = retry_chain(|| self.app.web_ensure_keys(&entry)).await {
            return Self::start_refused(&app_error(e), line);
        }
        let old = cookie(headers, super::SESSION_COOKIE);
        let opened = match self
            .app
            .web_session_open(&entry, self.pages().session_ttl_secs, old.as_deref())
            .await
        {
            Ok(opened) => opened,
            Err(e) => return Self::start_refused(&app_error(e), line),
        };
        let mut line = line;
        line.session(&opened.record);
        super::sign_in_line(
            request_id,
            pending.identity.provider,
            "signed_up",
            Some(&opened.record),
        );
        let mut response = self.redirect(
            "/web/start",
            Some(set_cookie(
                super::SESSION_COOKIE,
                &opened.token,
                self.pages().session_ttl_secs,
            )),
        );
        append_cookie(&mut response, &clear_cookie(SIGN_UP_COOKIE));
        Answer {
            response,
            error_code: None,
            line,
        }
    }

    // -- the callback's landing ---------------------------------------------

    /// The callback's redirect after an admitted sign-in (ADR-0068 d10):
    /// `/web/start` while the person holds no agent key, `/web` after.
    pub(crate) async fn landing(&self, entry: &oml_app::types::WebEntry) -> &'static str {
        match self.app.web_has_agent_key(entry).await {
            Ok(false) => "/web/start",
            _ => "/web",
        }
    }

    /// The callback's answer to an unlisted identity, or `None` where the
    /// refusal is another.
    pub(crate) fn unlisted(&self, refusal: &CallbackRefusal) -> Option<Response> {
        match refusal {
            CallbackRefusal::Unlisted(identity) => Some(self.terms_page(identity.clone())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_up_pages_policy_names_no_script() {
        assert!(!start_csp().contains("script-src"));
        assert!(start_csp().starts_with("default-src 'none'"));
    }

    #[test]
    fn the_compile_time_digests_are_sha256() {
        let d = ring::digest::digest(&ring::digest::SHA256, STYLESHEET.as_bytes());
        assert_eq!(base64_32(d.as_ref().try_into().unwrap()), STYLE_DIGEST);
    }

    #[test]
    fn the_address_limit_admits_its_count_an_hour() {
        let limiter = SignupLimiter::default();
        let ip: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        for _ in 0..5 {
            assert!(limiter.take(ip, 1000, 5).is_ok());
        }
        assert!(limiter.take(ip, 1000, 5).is_err());
        assert!(
            limiter
                .take(Some("203.0.113.10".parse().unwrap()), 1000, 5)
                .is_ok()
        );
        assert!(
            limiter.take(ip, 4600, 5).is_ok(),
            "an hour later a slot frees"
        );
    }

    #[test]
    fn addresses_of_one_ipv6_slash_64_share_a_bucket_and_an_ipv4_address_keeps_its_own() {
        let limiter = SignupLimiter::default();
        let a: Option<IpAddr> = Some("2001:db8:1:2::1".parse().unwrap());
        let same_64: Option<IpAddr> = Some("2001:db8:1:2:ffff:ffff:ffff:ffff".parse().unwrap());
        let other_64: Option<IpAddr> = Some("2001:db8:1:3::1".parse().unwrap());
        assert!(limiter.take(a, 1000, 1).is_ok());
        assert!(
            limiter.take(same_64, 1000, 1).is_err(),
            "a second host of the /64 is the same client"
        );
        assert!(limiter.take(other_64, 1000, 1).is_ok());
        // An IPv4 address and the same address mapped into IPv6 are one client.
        let v4: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        let mapped: Option<IpAddr> = Some("::ffff:203.0.113.9".parse().unwrap());
        assert!(limiter.take(v4, 1000, 1).is_ok());
        assert!(limiter.take(mapped, 1000, 1).is_err());
        assert!(
            limiter
                .take(Some("203.0.113.10".parse().unwrap()), 1000, 1)
                .is_ok(),
            "IPv4 is bucketed by the address itself"
        );
    }

    #[test]
    fn a_bucket_whose_slots_have_all_expired_is_dropped() {
        let limiter = SignupLimiter::default();
        for n in 0..50u8 {
            let ip: Option<IpAddr> = Some(format!("198.51.100.{n}").parse().unwrap());
            assert!(limiter.take(ip, 1000, 5).is_ok());
        }
        assert!(limiter.take(None, 5000, 5).is_ok());
        assert_eq!(limiter.buckets(), 1, "only the live bucket remains");
    }

    #[test]
    fn a_pending_sign_up_round_trips_and_expires() {
        let p = PendingSignUp {
            identity: ProviderIdentity {
                provider: Provider::GitHub,
                subject: "7".to_owned(),
                email: Email::parse("a@example.com").unwrap(),
            },
            started_at: 100,
        };
        assert_eq!(PendingSignUp::decode(&p.encode()), Some(p.clone()));
        assert!(p.live(100 + SIGN_IN_TTL_SECS - 1));
        assert!(!p.live(100 + SIGN_IN_TTL_SECS));
    }
}
