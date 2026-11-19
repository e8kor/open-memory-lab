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
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http::{HeaderMap, header};
use oml_page::web::{Bar, Scope, Todo};
use oml_sdk::{AdminTodoCounts, Call, TodoCounts, Whoami};
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
    csrf: OnceLock<Csrf>,
    path: OnceLock<String>,
    signed_in: OnceLock<(String, bool, Todo)>,
}

/// The CSRF token of the session a request's cookie names.
struct Csrf(String);

/// No debug output prints the token.
impl std::fmt::Debug for Csrf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
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
            csrf: OnceLock::new(),
            path: OnceLock::new(),
            signed_in: OnceLock::new(),
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

    /// Records the session cookie the request carries and the page's
    /// `path`, so a refusal page answered to it draws that session's
    /// sign-out control (#327) and its link back.
    pub fn carries_session(&self, token: &str, path: &str) {
        let _ = self.csrf.set(Csrf(oml_web_credential::csrf_token(token)));
        let _ = self.path.set(path.to_owned());
    }

    /// Records who the session is and whether it holds the admin capacity,
    /// once it resolved, so a refusal page is drawn on its side's bar.
    pub fn signed_in(&self, session: &Session) {
        let scope = Scope::of_path(self.path(), session.admin);
        let _ = self
            .signed_in
            .set((session.who(), session.admin, session.badge(scope)));
    }

    /// The page's path, `/web` until the request carried a session.
    #[must_use]
    pub fn path(&self) -> &str {
        self.path.get().map_or("/web", String::as_str)
    }

    /// The bar a refusal page is drawn in: none for a request without a
    /// session cookie; for one whose session the API did not answer, the
    /// Personal bar with no name and no switch; else the side of the page
    /// asked for where the session holds it, the Personal side where not.
    #[must_use]
    pub fn bar(&self) -> Option<Bar> {
        let csrf = self.csrf()?.to_owned();
        let (who, admin, todo) = self.signed_in.get().cloned().unwrap_or_default();
        Some(Bar {
            scope: Scope::of_path(self.path(), admin),
            who,
            csrf,
            todo,
        })
    }

    /// The CSRF token of the session the request's cookie names; `None`
    /// where the request carries no session cookie.
    #[must_use]
    pub fn csrf(&self) -> Option<&str> {
        self.csrf.get().map(|csrf| csrf.0.as_str())
    }
}

/// A resolved session: its token, the `web` capacity's record, and whether
/// the entry is an admin's (its `admin` credential resolves).
#[derive(Clone)]
pub struct Session {
    token: String,
    pub whoami: Whoami,
    pub admin: bool,
    /// The To do counts read as `<user>-web`, made on a Personal page's
    /// `GET` (ADR-0083 d6, d11); `None` where it was not made or did not
    /// answer.
    pub todo: Option<TodoCounts>,
    /// The administration counts read as `<user>-admin`, made on an admin
    /// page's `GET`; `None` likewise.
    pub admin_todo: Option<AdminTodoCounts>,
}

/// No debug output prints the session token.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("token", &"<redacted>")
            .field("whoami", &self.whoami)
            .field("admin", &self.admin)
            .field("todo", &self.todo)
            .finish()
    }
}

impl Session {
    /// The credential of `capacity`, derived from the session token.
    #[must_use]
    pub fn credential(&self, capacity: Capacity) -> String {
        capacity_credential(&self.token, capacity.as_str())
    }

    /// The session token, which seals a shown-once secret to this session
    /// ([`crate::once`]); never shown or logged.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// The Personal side's bar for this session, with the switch to
    /// Administration where it holds the admin capacity.
    #[must_use]
    pub fn personal(&self) -> Bar {
        self.bar(Scope::Personal { admin: self.admin })
    }

    /// The Administration side's bar for this session; drawn only once a
    /// page checked that it holds the admin capacity.
    #[must_use]
    pub fn administration(&self) -> Bar {
        self.bar(Scope::Administration)
    }

    fn bar(&self, scope: Scope) -> Bar {
        Bar {
            scope,
            who: self.who(),
            csrf: self.csrf(),
            todo: self.badge(scope),
        }
    }

    /// The To do badge of `scope`'s bar: the sum of the side's counts read
    /// (only `review` is served on the Personal side, and nothing on the
    /// admin side, ADR-0083 d14), or no number where the read was not made
    /// or did not answer, so the page never shows a number the API did not
    /// serve.
    #[must_use]
    pub fn badge(&self, scope: Scope) -> Todo {
        match scope {
            Scope::Personal { .. } => self.todo.map_or(Todo::Unread, |t| Todo::Counted {
                n: t.review,
                truncated: t.truncated,
            }),
            Scope::Administration => self.admin_todo.map_or(Todo::Unread, |t| Todo::Counted {
                n: 0,
                truncated: t.truncated,
            }),
        }
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

    /// The key a builder form's preview stamp is signed with: the act key
    /// of `preview/<nonce>`, derived apart from the `Idempotency-Key` the
    /// write is sent under (the review of PR #494).
    #[must_use]
    pub fn preview_key(&self, nonce: &str) -> String {
        self.act_key(&format!("preview/{nonce}"))
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
        acting_key: None,
    }
}

/// [`call`] acting as `acting`, the id of a tenant key an admin linked to
/// the session's person: the lead runs the request as that key, with its
/// grants, or refuses it `acting_key_not_linked` (ADR-0085 d16).
#[must_use]
pub fn call_as<'a>(credential: &'a str, ids: &'a CallIds, acting: Option<&'a str>) -> Call<'a> {
    Call {
        acting_key: acting,
        ..call(credential, ids)
    }
}

/// Layer 3: the cookie's `web` credential checked by `GET /v2/whoami`, and
/// beside it the `admin` credential's, which resolves only for an admin's
/// entry; both in one round trip. No cookie, or `auth.invalid`, is the
/// `303` to sign in; any other failure is the page of layer 7. From the
/// cookie on, every refusal page the request is answered carries the
/// sign-out control under the cookie's CSRF token, the page of a check
/// that had no answer included: the lead's act decides whether a session
/// stands behind it (#327).
pub async fn load(State(web): State<Web>, mut req: Request, next: Next) -> Response {
    // A builder post's session, resolved before its body was read
    // (`middleware::same_origin`), is not resolved again.
    if req.extensions().get::<Session>().is_none() {
        let calls = page_calls(req.extensions());
        match resolve_request(&web, &calls, req.headers(), req.uri()).await {
            Ok(session) => {
                req.extensions_mut().insert(session);
            }
            Err(answer) => return *answer,
        }
    }
    next.run(req).await
}

/// The page's record of its calls, which layer 1 puts on the request.
pub(crate) fn page_calls(extensions: &http::Extensions) -> Arc<PageCalls> {
    extensions
        .get::<Arc<PageCalls>>()
        .cloned()
        .unwrap_or_else(|| Arc::new(PageCalls::new("page")))
}

/// Layer 3's read of the session a request carries, or the page's answer
/// when there is none: sign in without a cookie, and the whoami's refusal
/// for a session that does not resolve.
pub(crate) async fn resolve_request(
    web: &Web,
    calls: &PageCalls,
    headers: &HeaderMap,
    uri: &http::Uri,
) -> Result<Session, Box<Response>> {
    let page = uri
        .path_and_query()
        .map_or_else(|| "/web".to_owned(), |pq| pq.as_str().to_owned());
    let Some(token) = cookie(headers, SESSION_COOKIE) else {
        return Err(Box::new(outcome::sign_in(&page)));
    };
    calls.carries_session(&token, uri.path());
    // The badge's counts read, beside whoami in one round, on every page
    // request: a form's post that draws a page (Preview, a refusal, a
    // rebase) carries the number as a GET does (ADR-0083 d6; the review of
    // PR #494, 11).
    let counts = Some(Scope::of_path(uri.path(), true));
    let session = resolve_with_counts(web, calls, token, counts)
        .await
        .map_err(|e| Box::new(outcome::page(&e, calls, &page)))?;
    calls.signed_in(&session);
    Ok(session)
}

/// The session `token` names, checked by `GET /v2/whoami` under its `web`
/// credential, with whether its `admin` credential resolves; both in one
/// round trip.
///
/// # Errors
/// The `web` credential's whoami failed: the API's refusal, `auth.invalid`
/// for a session that is gone, or no usable answer.
pub async fn resolve(
    web: &Web,
    calls: &PageCalls,
    token: String,
) -> Result<Session, oml_sdk::Error> {
    resolve_with_counts(web, calls, token, None).await
}

/// [`resolve`], and with `counts` the badge's counts read of that side in
/// the same round: one call per page view, `GET /v2/todo/counts` as
/// `<user>-web` or `GET /v2/admin/todo/counts` as `<user>-admin`
/// (ADR-0083 d6). A read that fails leaves the badge without a number and
/// the page drawn whole: the count is a hint, and the To do page itself
/// reads the list.
async fn resolve_with_counts(
    web: &Web,
    calls: &PageCalls,
    token: String,
    counts: Option<Scope>,
) -> Result<Session, oml_sdk::Error> {
    let web_credential = capacity_credential(&token, Capacity::Web.as_str());
    let admin_credential = capacity_credential(&token, Capacity::Admin.as_str());
    let (web_ids, admin_ids) = (calls.next_ids(), calls.next_ids());
    let (web_call, admin_call) = (
        call(&web_credential, &web_ids),
        call(&admin_credential, &admin_ids),
    );
    let counts_ids = counts.map(|_| calls.next_ids());
    let read = async {
        match (counts, &counts_ids) {
            (Some(Scope::Personal { .. }), Some(ids)) => (
                web.api
                    .todo_counts(&call(&web_credential, ids), None)
                    .await
                    .ok(),
                None,
            ),
            (Some(Scope::Administration), Some(ids)) => (
                None,
                web.api
                    .admin_todo_counts(&call(&admin_credential, ids))
                    .await
                    .ok(),
            ),
            _ => (None, None),
        }
    };
    let (me, admin, (todo, admin_todo)) =
        tokio::join!(web.api.whoami(&web_call), web.api.whoami(&admin_call), read);
    let whoami = me?;
    calls.acting_as(whoami.name.as_deref());
    Ok(Session {
        token,
        whoami,
        admin: admin.is_ok(),
        todo,
        admin_todo,
    })
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
                denied: None,
            },
            admin: false,
            todo: None,
            admin_todo: None,
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
