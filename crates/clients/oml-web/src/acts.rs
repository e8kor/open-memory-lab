//! What every act of a moved page shares (ADR-0070 d3 layer 4), as the
//! Workareas view's acts do it: the form read once with each field at most
//! once, an action from the page's closed set and no field it does not
//! define, the form's nonce as the act's `Idempotency-Key`, a second send
//! of the same form waiting for the first, and a success answered with a
//! `303` (Post/Redirect/Get).

use axum::extract::Form;
use axum::extract::rejection::FormRejection;
use axum::response::{IntoResponse as _, Response};
use http::{HeaderValue, StatusCode, header};

use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session};
use crate::workareas::{in_flight, is_nonce, nonce};

/// A page that ends an act or a view early, boxed, since a response is
/// large beside what it stands in for.
pub(crate) type Refusal = Box<Response>;

/// A `303` to `location`, with each of `cookies` set.
pub(crate) fn see_other(location: &str, cookies: &[String]) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// A refusal of the form itself, before any API call.
pub(crate) fn invalid(message: &str, calls: &PageCalls) -> Response {
    refused(
        StatusCode::BAD_REQUEST,
        "request.validation",
        message,
        calls,
    )
}

/// An act's form: its fields, its action out of `actions`, recorded as the
/// line's `web_action`, and no field outside what `defined` names for it
/// beside `action`, `csrf_token` and `nonce`.
pub(crate) fn read<'a>(
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
    actions: &[&'a str],
    defined: impl Fn(&str, &str) -> bool,
    calls: &PageCalls,
) -> Result<(Fields, &'a str), Refusal> {
    let fields = form
        .map_err(|e| e.body_text())
        .and_then(|Form(pairs)| Fields::of_pairs(pairs))
        .map_err(|message| Box::new(invalid(&message, calls)))?;
    let Some(action) = fields
        .get("action")
        .and_then(|a| actions.iter().find(|x| **x == a).copied())
    else {
        return Err(Box::new(invalid(
            "action is not one of this page's actions",
            calls,
        )));
    };
    calls.act(action);
    if let Some(name) = fields
        .names()
        .find(|name| !matches!(*name, "action" | "csrf_token" | "nonce") && !defined(action, name))
    {
        return Err(Box::new(invalid(
            &format!("{action} takes no field {name}"),
            calls,
        )));
    }
    Ok((fields, action))
}

/// The act's `Idempotency-Key`: the digest of the form's nonce with the
/// session token, so a resubmitted form replays; a form without a nonce
/// acts once under a fresh key, and a nonce the page did not draw is
/// refused, so a mangled form never loses its replay silently.
pub(crate) fn key(
    session: &Session,
    fields: &Fields,
    calls: &PageCalls,
) -> Result<String, Refusal> {
    let nonce = match fields.get("nonce") {
        Some(sent) if is_nonce(sent) => sent.to_owned(),
        Some(_) => {
            return Err(Box::new(invalid(
                "nonce is not one this page draws: 32 lower-case hexadecimal characters",
                calls,
            )));
        }
        None => nonce(),
    };
    Ok(session.act_key(&nonce))
}

/// Held while the act under `key` is in flight in this process, so a
/// second send of the same form (a double click) waits and then replays.
pub(crate) async fn hold(key: &str) -> tokio::sync::OwnedMutexGuard<()> {
    in_flight(key).lock_owned().await
}

/// A fresh nonce as a form's hidden field.
pub(crate) fn nonce_field() -> String {
    oml_page::web::hidden("nonce", &nonce())
}
