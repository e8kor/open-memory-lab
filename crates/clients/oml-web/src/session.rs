//! Layer 3 of ADR-0070 d3, the session load, and what a page's API calls
//! carry. `oml-web` reads the `__Host-oml_session` cookie and derives the
//! capacity credentials from it (d2); it never judges a session itself:
//! the first API call, `GET /v2/whoami`, is the check, and no cookie or
//! `auth.invalid` sends the person to sign in.
//!
//! Every API call a page makes carries `X-Correlation-Id` set to the page's
//! request id and `X-Request-Id` `<page id>.<n>` (d3 layer 1, ADR-0042 d4),
//! so one filter finds the page's line and its API lines.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http::{HeaderMap, header};
use oml_sdk::{Call, Whoami};
use oml_web_credential::capacity_credential;

use crate::{Capacity, SESSION_COOKIE, Web, outcome};

/// The value of the cookie `name`, from the one or several `Cookie`
/// headers; `None` when it is absent or not visible ASCII. The lead's
/// reading, so both processes see the same session.
#[must_use]
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_graphic()))
}

/// One page request's API calls: the ids each carries, the acting key the
/// page resolved, and the request ids made, for the page's line.
#[derive(Debug)]
pub struct PageCalls {
    page_id: String,
    next: AtomicUsize,
    made: Mutex<Vec<String>>,
    acting: Mutex<Option<String>>,
    action: Mutex<Option<String>>,
}

/// The ids of one API call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallIds {
    pub request_id: Option<String>,
    pub correlation_id: String,
}

impl PageCalls {
    /// The calls of the page whose request id is `page_id`.
    #[must_use]
    pub fn new(page_id: &str) -> Self {
        Self {
            page_id: page_id.to_owned(),
            next: AtomicUsize::new(0),
            made: Mutex::new(Vec::new()),
            acting: Mutex::new(None),
            action: Mutex::new(None),
        }
    }

    /// The page's request id.
    #[must_use]
    pub fn page_id(&self) -> &str {
        &self.page_id
    }

    /// The next call's ids: `<page id>.<n>`, counted from 1, and the page's
    /// id as the correlation. Where the page id is so long that the suffix
    /// would break the id rule, the call names none and the API mints one.
    pub fn next_ids(&self) -> CallIds {
        let n = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let id = format!("{}.{n}", self.page_id);
        let request_id = oml_page::request_id_admitted(id.as_bytes()).then_some(id);
        if let Some(id) = &request_id {
            self.made.lock().expect("unpoisoned").push(id.clone());
        }
        CallIds {
            request_id,
            correlation_id: self.page_id.clone(),
        }
    }

    /// The request ids of the calls made so far.
    #[must_use]
    pub fn made(&self) -> Vec<String> {
        self.made.lock().expect("unpoisoned").clone()
    }

    /// Records the key the page acts as.
    pub fn acting_as(&self, key: Option<&str>) {
        *self.acting.lock().expect("unpoisoned") = key.map(str::to_owned);
    }

    /// The key the page acted as, once its session resolved.
    #[must_use]
    pub fn acting(&self) -> Option<String> {
        self.acting.lock().expect("unpoisoned").clone()
    }

    /// Records the form's `action`, the page line's `web_action`.
    pub fn act(&self, action: &str) {
        *self.action.lock().expect("unpoisoned") = Some(action.to_owned());
    }

    /// The form's `action`, where the request was an act.
    #[must_use]
    pub fn action(&self) -> Option<String> {
        self.action.lock().expect("unpoisoned").clone()
    }
}

/// A resolved session: its token, the `web` capacity's record, and whether
/// the entry is an admin's (its `admin` credential resolves).
#[derive(Clone)]
pub struct Session {
    token: String,
    pub whoami: Whoami,
    pub admin: bool,
}

/// No debug output prints the session token.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("token", &"<redacted>")
            .field("whoami", &self.whoami)
            .field("admin", &self.admin)
            .finish()
    }
}

impl Session {
    /// The credential of `capacity`, derived from the session token.
    #[must_use]
    pub fn credential(&self, capacity: Capacity) -> String {
        capacity_credential(&self.token, capacity.as_str())
    }

    /// The session's CSRF token, which every form of its pages carries
    /// (ADR-0045 d8).
    #[must_use]
    pub fn csrf(&self) -> String {
        oml_web_credential::csrf_token(&self.token)
    }

    /// The `Idempotency-Key` of the act a form with `nonce` makes
    /// (ADR-0070 d3 layer 4).
    #[must_use]
    pub fn act_key(&self, nonce: &str) -> String {
        oml_web_credential::act_idempotency_key(&self.token, nonce)
    }

    /// The header's "who": the person and their organisation, as the lead's
    /// pages name them.
    #[must_use]
    pub fn who(&self) -> String {
        let user = self.whoami.user.as_ref().map_or("", |u| u.name.as_str());
        match &self.whoami.tenant {
            Some(org) => format!("{user} · {}", org.name),
            None => user.to_owned(),
        }
    }
}

/// An API call's `Call` from its credential and ids.
#[must_use]
pub fn call<'a>(credential: &'a str, ids: &'a CallIds) -> Call<'a> {
    Call {
        bearer: credential,
        request_id: ids.request_id.as_deref(),
        correlation_id: Some(&ids.correlation_id),
    }
}

/// Layer 3: the cookie's `web` credential checked by `GET /v2/whoami`, and
/// beside it the `admin` credential's, which resolves only for an admin's
/// entry; both in one round trip. No cookie, or `auth.invalid`, is the
/// `303` to sign in; any other failure is the page of layer 7.
pub async fn load(State(web): State<Web>, mut req: Request, next: Next) -> Response {
    let calls = req
        .extensions()
        .get::<Arc<PageCalls>>()
        .cloned()
        .unwrap_or_else(|| Arc::new(PageCalls::new("page")));
    let page = req
        .uri()
        .path_and_query()
        .map_or_else(|| "/web".to_owned(), |pq| pq.as_str().to_owned());
    let Some(token) = cookie(req.headers(), SESSION_COOKIE) else {
        return outcome::sign_in(&page);
    };
    let web_credential = capacity_credential(&token, Capacity::Web.as_str());
    let admin_credential = capacity_credential(&token, Capacity::Admin.as_str());
    let (web_ids, admin_ids) = (calls.next_ids(), calls.next_ids());
    let (web_call, admin_call) = (
        call(&web_credential, &web_ids),
        call(&admin_credential, &admin_ids),
    );
    let (me, admin) = tokio::join!(web.api.whoami(&web_call), web.api.whoami(&admin_call));
    let whoami = match me {
        Ok(whoami) => whoami,
        Err(e) => return outcome::page(&e, calls.page_id(), &page),
    };
    calls.acting_as(whoami.name.as_deref());
    req.extensions_mut().insert(Session {
        token,
        whoami,
        admin: admin.is_ok(),
    });
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_call_of_a_page_is_numbered_under_the_pages_id() {
        let calls = PageCalls::new("page-1");
        assert_eq!(
            calls.next_ids(),
            CallIds {
                request_id: Some("page-1.1".to_owned()),
                correlation_id: "page-1".to_owned(),
            }
        );
        assert_eq!(calls.next_ids().request_id.as_deref(), Some("page-1.2"));
        assert_eq!(calls.made(), ["page-1.1", "page-1.2"]);
    }

    #[test]
    fn a_page_id_too_long_for_a_suffix_lets_the_api_mint_the_calls_id() {
        let long = "p".repeat(128);
        let calls = PageCalls::new(&long);
        let ids = calls.next_ids();
        assert_eq!(ids.request_id, None);
        assert_eq!(ids.correlation_id, long);
    }

    #[test]
    fn a_sessions_debug_output_never_prints_its_token() {
        let session = Session {
            token: "the-secret-session-token".to_owned(),
            whoami: Whoami {
                key_id: "k".to_owned(),
                name: Some("alice-web".to_owned()),
                role: "user".to_owned(),
                tenant: None,
                user: None,
            },
            admin: false,
        };
        let shown = format!("{session:?}");
        assert!(!shown.contains("the-secret-session-token"), "{shown}");
        assert!(
            !shown.contains(&session.credential(Capacity::Web)),
            "{shown}"
        );
    }

    #[test]
    fn the_cookie_is_read_as_the_lead_reads_it() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            "a=1; __Host-oml_session=tok".parse().unwrap(),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE).as_deref(), Some("tok"));
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, "__Host-oml_session=".parse().unwrap());
        assert_eq!(cookie(&headers, SESSION_COOKIE), None);
    }
}
