//! Layer 7 of ADR-0070 d3: the API's error envelope becomes a page outcome.
//! A pure table, so every code the pages can meet is answered in one place
//! and a code the table does not name is the generic refusal; then
//! [`page`] answers the outcome as a page.

use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use oml_sdk::{Error, TransportError};

use crate::session::PageCalls;

/// What a page shows for an API refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOutcome {
    /// `access.not_found`: the not-found page.
    NotFound,
    /// `auth.invalid`: the session is gone, so the sign-in page.
    SignIn,
    /// `request.validation`: the form again, with `details.field` marked.
    FormAgain,
    /// `limit.*`: the retry page, with the API's `Retry-After`.
    Retry,
    /// `review.*` and `workarea.*`: the page itself, with the API's message.
    Message,
    /// Any other code: the generic refusal page, with the request id.
    Refused,
}

impl PageOutcome {
    /// The outcome of the envelope's `code`.
    #[must_use]
    pub fn of_code(code: &str) -> Self {
        match code {
            "access.not_found" => Self::NotFound,
            "auth.invalid" => Self::SignIn,
            "request.validation" => Self::FormAgain,
            _ if code.starts_with("limit.") => Self::Retry,
            _ if code.starts_with("review.") || code.starts_with("workarea.") => Self::Message,
            _ => Self::Refused,
        }
    }

    /// The status the page answers with: the sign-in outcome is a `303` to
    /// the sign-in page (ADR-0070 d3 layer 3); every other keeps the API's
    /// own status, so a page refuses with the status the lead's page did.
    #[must_use]
    pub fn status(self, api_status: StatusCode) -> StatusCode {
        match self {
            Self::SignIn => StatusCode::SEE_OTHER,
            _ => api_status,
        }
    }
}

/// The outcome of a whole envelope body, `{"error": {"code": …}}`. A body
/// that is not an envelope is the generic refusal: the page never guesses.
#[must_use]
pub fn of_envelope(body: &[u8]) -> PageOutcome {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["code"].as_str().map(PageOutcome::of_code))
        .unwrap_or(PageOutcome::Refused)
}

/// The `303` to the lead's sign-in page, carrying `next`, the page the
/// person asked for (ADR-0070 d4).
#[must_use]
pub fn sign_in(next: &str) -> Response {
    let location = oml_page::sign_in_location(next);
    let location = HeaderValue::from_str(&location)
        .unwrap_or_else(|_| HeaderValue::from_static(oml_page::SIGN_IN_PATH));
    (StatusCode::SEE_OTHER, [(header::LOCATION, location)]).into_response()
}

/// An HTML answer with `status`.
#[must_use]
pub fn html(status: StatusCode, body: String) -> Response {
    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

/// The refusal page of `code` and `message`, naming the page's request id
/// and drawn in the bar of the session `calls` records, with its sign-out
/// control and a link back to the page's tab (#327; design notes row 25).
#[must_use]
pub fn refusal_page(code: &str, message: &str, calls: &PageCalls) -> String {
    oml_page::web::refusal_page(
        code,
        message,
        Some(calls.page_id()),
        calls.bar().as_ref(),
        calls.path(),
    )
}

/// The page for a call that has no usable answer, naming the page's
/// request id and carrying the sign-out control of the session `calls`
/// records: an API refusal as its outcome, a timeout as `504`, and an
/// unreachable API or an answer not in the operation's shape as `502`
/// (ADR-0070 d3 layer 7, d8; #327). `next` is the page, for the sign-in
/// outcome.
#[must_use]
pub fn page(error: &Error, calls: &PageCalls, next: &str) -> Response {
    let refusal = |code: &str, message: &str| refusal_page(code, message, calls);
    match error {
        Error::Api(e) => {
            let outcome = PageOutcome::of_code(&e.code);
            if outcome == PageOutcome::SignIn {
                return sign_in(next);
            }
            let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = html(outcome.status(status), refusal(&e.code, &e.message));
            if outcome == PageOutcome::Retry
                && let Some(after) = e
                    .retry_after
                    .as_deref()
                    .and_then(|v| HeaderValue::from_str(v).ok())
            {
                response.headers_mut().insert(header::RETRY_AFTER, after);
            }
            response
        }
        Error::Transport(TransportError::Timeout) => html(
            StatusCode::GATEWAY_TIMEOUT,
            refusal(
                "unavailable",
                "the server did not answer this page in time; try again",
            ),
        ),
        Error::Transport(TransportError::Unreachable(_)) | Error::Decode { .. } => html(
            StatusCode::BAD_GATEWAY,
            refusal(
                "unavailable",
                "the server could not answer this page; try again",
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use oml_sdk::ApiError;

    use super::*;

    #[test]
    fn each_code_of_the_d3_table_maps_to_its_page_outcome() {
        let table = [
            ("access.not_found", PageOutcome::NotFound),
            ("auth.invalid", PageOutcome::SignIn),
            ("request.validation", PageOutcome::FormAgain),
            ("limit.rate", PageOutcome::Retry),
            ("limit.quota", PageOutcome::Retry),
            ("review.self_review", PageOutcome::Message),
            ("workarea.not_empty", PageOutcome::Message),
            ("access.verb_ceiling", PageOutcome::Refused),
            ("something.new", PageOutcome::Refused),
        ];
        for (code, want) in table {
            assert_eq!(PageOutcome::of_code(code), want, "{code}");
        }
    }

    #[test]
    fn a_body_that_is_not_an_envelope_is_the_generic_refusal() {
        assert_eq!(of_envelope(b"<html>"), PageOutcome::Refused);
        assert_eq!(
            of_envelope(br#"{"code":"auth.invalid"}"#),
            PageOutcome::Refused
        );
        assert_eq!(
            of_envelope(br#"{"error":{"code":"auth.invalid"},"request_id":"req_1"}"#),
            PageOutcome::SignIn
        );
    }

    #[test]
    fn the_sign_in_outcome_redirects_and_every_other_keeps_the_apis_status() {
        assert_eq!(
            PageOutcome::SignIn.status(StatusCode::UNAUTHORIZED),
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            PageOutcome::Refused.status(StatusCode::FORBIDDEN),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            PageOutcome::FormAgain.status(StatusCode::BAD_REQUEST),
            StatusCode::BAD_REQUEST
        );
    }

    fn api(status: u16, code: &str) -> Error {
        Error::Api(Box::new(ApiError {
            status,
            code: code.to_owned(),
            message: "the message".to_owned(),
            details: serde_json::Value::Null,
            request_id: Some("req_api".to_owned()),
            retry_after: Some("30".to_owned()),
        }))
    }

    #[test]
    fn each_failure_answers_its_page_status() {
        let calls = PageCalls::new("page-7");
        let response = page(&api(404, "access.not_found"), &calls, "/web");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = page(&api(429, "limit.rate"), &calls, "/web");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "30");
        let response = page(&api(401, "auth.invalid"), &calls, "/web/guardrails");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers()[header::LOCATION],
            "/web/sign-in?next=%2Fweb%2Fguardrails"
        );
        let response = page(&Error::Transport(TransportError::Timeout), &calls, "/web");
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let response = page(
            &Error::Transport(TransportError::Unreachable("refused".to_owned())),
            &calls,
            "/web",
        );
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    /// Every failure's page carries the sign-out control once the request
    /// carried a session cookie, a session check that had no answer
    /// included, and none before (#327).
    #[tokio::test]
    async fn a_failure_page_draws_the_sign_out_control_once_the_request_carries_a_session() {
        let failures = [
            api(404, "access.not_found"),
            api(429, "limit.rate"),
            Error::Transport(TransportError::Timeout),
            Error::Transport(TransportError::Unreachable("refused".to_owned())),
        ];
        let body = |response: Response| async {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("a body");
            String::from_utf8(bytes.to_vec()).expect("UTF-8")
        };
        let control = format!(
            r#"name="action" value="sign_out"><input type="hidden" name="csrf_token" value="{}">"#,
            oml_web_credential::csrf_token("the-session-token")
        );
        for failure in &failures {
            let calls = PageCalls::new("page-7");
            let no_one = body(page(failure, &calls, "/web")).await;
            assert!(!no_one.contains("sign_out"), "{failure:?}");
            calls.carries_session("the-session-token", "/web/keys");
            let signed_in = body(page(failure, &calls, "/web")).await;
            assert!(signed_in.contains(&control), "{failure:?}");
        }
    }
}
